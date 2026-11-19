import { ProcessTerminal } from "./tui-core/terminal.js";
import { DifferentialRenderer } from "./tui-core/differential.js";
import { colors, visualWidth } from "./tui-core/colors.js";
import { parseKey } from "./tui-core/keys.js";
import { Editor } from "./components/editor.js";
import { StreamingMarkdown } from "./components/markdown.js";
import { Spinner, StatusBar } from "./components/status.js";
import { ApprovalDialog, ApprovalDecision } from "./components/approval.js";
import { ModelSelector } from "./components/model-selector.js";
import { SlashMenu, SlashCommandItem } from "./components/slash-menu.js";
import { ThinkBlock } from "./components/think-block.js";
import { AnankeClient, EventItem } from "./client.js";

export type TimelineItem =
  | { type: "user"; text: string }
  | { type: "think"; block: ThinkBlock }
  | { type: "text"; text: string }
  | { type: "tool"; tool: string; status: "running" | "done" | "failed"; detail?: string }
  | { type: "system"; text: string };

/**
 * Ananke TUI Application Runner (AD-12)
 * Kế thừa kiến trúc Differential Rendering, chống giật, mượt mà và headless.
 */
class AnankeTuiApp {
  private terminal: ProcessTerminal;
  private renderer: DifferentialRenderer;
  private client: AnankeClient;

  // Components
  private statusBar: StatusBar;
  private spinner: Spinner;
  private editor: Editor;
  private activeApproval: ApprovalDialog | null = null;
  private activeModelSelector: ModelSelector | null = null;
  private slashMenu: SlashMenu | null = null;

  private availableCommands: SlashCommandItem[] = [
    { name: "help", description: "Hiển thị trợ giúp danh sách lệnh" },
    { name: "model", description: "Chọn model từ danh sách nhà cung cấp" },
    { name: "clear", description: "Xóa toàn bộ màn hình lịch sử hội thoại" },
    { name: "interrupt", description: "Ngắt lượt suy nghĩ hiện tại của agent" },
    { name: "perm", description: "Thay đổi chế độ quyền hạn của phiên" },
    { name: "exit", description: "Thoát ứng dụng TUI (hoặc Ctrl+D/Q)" },
  ];

  // State
  private sessionId = "";
  private currentTurnId: string | null = null;
  private activeModel = "default";
  private permissionMode = "WorkspaceWrite";
  private timeline: TimelineItem[] = [];
  private currentThinking: ThinkBlock | null = null;
  private currentStreamingText = "";
  private activePartKey: string | null = null;
  private activePartContent = "";
  private unsubscribeEvents: (() => void) | null = null;
  private spinnerTimer: ReturnType<typeof setInterval> | null = null;
  private thinkLineRanges: { start: number; end: number; block: ThinkBlock }[] = [];

  constructor(serverUrl: string = "http://127.0.0.1:9876") {
    this.terminal = new ProcessTerminal();
    this.renderer = new DifferentialRenderer({ terminal: this.terminal, clearOnShrink: false });
    this.client = new AnankeClient(serverUrl);

    this.statusBar = new StatusBar({
      modelName: this.activeModel,
      sandboxMode: "restricted-token",
      permissionMode: this.permissionMode,
    });

    this.spinner = new Spinner();
    this.editor = new Editor({
      modelBadge: this.activeModel,
      onSubmit: (text) => this.handleUserSubmit(text),
      onChange: (text) => this.handleEditorChange(text),
    });
  }

  async start(): Promise<void> {
    this.terminal.start(
      (data) => this.handleInput(data),
      () => {
        this.renderer.reset();
        this.requestRender();
      }
    );

    this.terminal.setTitle("Ananke TUI");
    this.terminal.hideCursor();

    // Khởi động spinner tick timer
    this.spinnerTimer = setInterval(() => {
      if (this.spinner.isActive()) {
        this.spinner.tick();
        this.requestRender();
      }
    }, 80);

    // Kết nối & tải metadata
    try {
      const meta = await this.client.getMeta();
      this.statusBar.update({
        sandboxMode: meta.sandbox,
      });

      // Nạp danh sách model để chọn model đầu tiên nếu chưa có
      try {
        const available = await this.client.listModels();
        if (available.length > 0) {
          const nonDefault = available.find((m) => m.id !== "default") || available[0];
          this.activeModel = nonDefault.id;
          this.statusBar.update({ modelName: this.activeModel });
          this.editor.setModelBadge(this.activeModel);
        }
      } catch {
        // Giữ default nếu không lấy được
      }

      const session = await this.client.createSession(process.cwd(), this.activeModel);
      this.sessionId = session.id;
      this.permissionMode = session.permissionMode || "WorkspaceWrite";
      this.statusBar.update({
        permissionMode: this.permissionMode,
      });

      this.appendLog(colors.green(`✔ Đã kết nối Ananke Server (v${meta.serverVersion})`));
      this.appendLog(colors.dim(`Session: ${session.id} | Profile: ${meta.deviceProfile}`));
      if (meta.sandbox === "none") {
        this.appendLog(colors.yellow(`⚠ Cảnh báo: Sandbox không khả dụng trên hệ điều hành này. Kiểm soát quyền đang bật.`));
      }

      this.subscribeToSession();
    } catch (err: any) {
      this.appendLog(colors.red(`✖ Lỗi kết nối server: ${err.message}`));
      this.appendLog(colors.dim(`Hãy chắc chắn \`cargo run -p ananke-server\` đang chạy tại port 9876.`));
    }

    this.requestRender();
  }

  private subscribeToSession(): void {
    if (!this.sessionId) return;
    this.unsubscribeEvents = this.client.subscribeEvents(
      this.sessionId,
      (event: EventItem) => this.handleServerEvent(event),
      (err: any) => {
        this.appendLog(colors.dim(`[SSE reconnecting: ${err?.message || "connection drop"}]`));
      }
    );
  }

  private handleServerEvent(event: EventItem): void {
    const type = event.type;
    if (!type) return;

    if (type === "part.delta") {
      const kind = event.kind;
      const isReasoning = kind === "Reasoning" || kind === "reasoning";
      const delta = String(event.delta ?? event.text ?? "");

      if (isReasoning) {
        if (!this.currentThinking) {
          this.currentThinking = new ThinkBlock();
          this.timeline.push({ type: "think", block: this.currentThinking });
        }
        this.currentThinking.append(delta);
      } else {
        this.currentStreamingText += delta;
      }
      this.requestRender();
    } else if (type === "turn.started") {
      this.currentTurnId = event.turnID || null;
      this.currentThinking = null;
      this.currentStreamingText = "";
      this.activePartKey = null;
      this.activePartContent = "";
      this.spinner.start("Agent đang suy nghĩ & xử lý...");
      this.requestRender();
    } else if (type === "tool.invoking") {
      // Đóng các stream dang dở
      if (this.currentStreamingText.trim()) {
        this.timeline.push({ type: "text", text: this.currentStreamingText.trim() });
        this.currentStreamingText = "";
      }
      this.currentThinking = null;

      const toolName = event.tool || "unknown";
      this.timeline.push({ type: "tool", tool: toolName, status: "running" });
      this.spinner.setMessage(`Đang thực thi tool: ${colors.cyan(toolName)}`);
      this.requestRender();
    } else if (type === "turn.completed") {
      this.spinner.stop();
      this.currentTurnId = null;

      if (this.currentStreamingText.trim()) {
        this.timeline.push({ type: "text", text: this.currentStreamingText.trim() });
        this.currentStreamingText = "";
      }
      this.currentThinking = null;
      this.requestRender();
    } else if (type === "turn.failed") {
      this.spinner.stop();
      this.currentTurnId = null;
      this.currentThinking = null;
      this.currentStreamingText = "";
      this.timeline.push({ type: "system", text: colors.red(`✖ Lượt xử lý thất bại: ${event.reason || "unknown error"}`) });
      this.requestRender();
    } else if (type === "permission.requested") {
      const permId = event.permissionID;
      this.activeApproval = new ApprovalDialog({
        request: {
          toolCallId: permId,
          toolName: event.tool || "unknown",
          arguments: event.arguments,
          reason: event.reason,
        },
        onDecision: async (decision: ApprovalDecision) => {
          this.activeApproval = null;
          try {
            await this.client.resolvePermission(this.sessionId, permId, decision);
            this.timeline.push({ type: "system", text: colors.dim(`Đã phản hồi quyền [${decision}] cho tool ${event.tool}`) });
          } catch (e: any) {
            this.timeline.push({ type: "system", text: colors.red(`Lỗi khi giải quyết quyền: ${e.message}`) });
          }
          this.requestRender();
        },
      });
      this.requestRender();
    }
  }

  private handleEditorChange(text: string): void {
     const trimmed = text.trim();
     if (trimmed.startsWith("/")) {
       if (!this.slashMenu) {
         this.slashMenu = new SlashMenu({
           commands: this.availableCommands,
           filter: trimmed,
           onSelect: (cmdName: string) => {
             this.editor.setText(cmdName + " ");
             this.slashMenu = null;
             this.requestRender();
           },
           onClose: () => {
             this.slashMenu = null;
             this.requestRender();
           },
         });
       } else {
         this.slashMenu.setFilter(trimmed);
       }
     } else {
       this.slashMenu = null;
     }
     this.requestRender();
   }

   private handleInput(data: string): void {
    // Xử lý sự kiện click chuột SGR: \x1b[<0;col;rowM (chuột trái click)
    const mouseMatch = data.match(/^\x1b\[<(\d+);(\d+);(\d+)([Mm])/);
    if (mouseMatch) {
      const button = parseInt(mouseMatch[1], 10);
      const row = parseInt(mouseMatch[3], 10) - 1; // 0-based row
      const isRelease = mouseMatch[4] === "m";

      // Chuột trái nhấn vào một khối Thinking
      if (button === 0 && !isRelease && this.thinkLineRanges.length > 0) {
        for (const range of this.thinkLineRanges) {
          if (row >= range.start && row < range.end) {
            range.block.toggle();
            this.requestRender();
            return;
          }
        }
      }
      return;
    }

    const key = parseKey(data);

    // Ctrl+D hoặc Ctrl+Q: Thoát app
    if (key.ctrl && (key.name === "d" || key.name === "q")) {
      this.stop();
      return;
    }

    // Ctrl+C: Xem như phím Copy (giữ nguyên không thoát)
    if (key.ctrl && key.name === "c") {
      return;
    }

    // Ctrl+T đóng/mở khối Thinking gần nhất
    if (key.ctrl && key.name === "t") {
      // Tìm khối think gần nhất trong timeline
      for (let i = this.timeline.length - 1; i >= 0; i--) {
        const item = this.timeline[i];
        if (item.type === "think") {
          item.block.toggle();
          this.requestRender();
          return;
        }
      }
      return;
    }

    // Nếu đang hiển thị modal ApprovalDialog -> ưu tiên dialog nhận input
    if (this.activeApproval) {
      if (this.activeApproval.handleInput(data)) {
        this.requestRender();
        return;
      }
    }

    // Nếu đang mở ModelSelector -> ưu tiên ModelSelector nhận input
    if (this.activeModelSelector) {
      if (this.activeModelSelector.handleInput(data)) {
        this.requestRender();
        return;
      }
    }

    // Nếu đang mở SlashMenu -> ưu tiên phím điều hướng Up/Down/Tab/Enter/Esc
    if (this.slashMenu) {
      const key = parseKey(data);
      if (key.name === "up" || key.name === "down" || key.name === "tab" || (key.name === "enter" && this.editor.getText().startsWith("/"))) {
        if (this.slashMenu.handleInput(data)) {
          this.requestRender();
          return;
        }
      } else if (key.name === "escape") {
        this.slashMenu = null;
        this.requestRender();
        return;
      }
    }

    // Input cho Editor
    if (this.editor.handleInput(data)) {
      this.requestRender();
    }
  }

  private async handleUserSubmit(text: string): Promise<void> {
    const trimmed = text.trim();
    if (!trimmed) return;

    // Xử lý các Slash Commands
    if (trimmed.startsWith("/")) {
      await this.handleSlashCommand(trimmed);
      return;
    }

    // Thêm tin nhắn của User vào timeline
    this.timeline.push({ type: "user", text: trimmed });
    this.requestRender();

    if (!this.sessionId) {
      this.timeline.push({ type: "system", text: colors.red("Chưa có phiên làm việc với server.") });
      this.requestRender();
      return;
    }

    try {
      await this.client.sendTurn(this.sessionId, trimmed);
    } catch (e: any) {
      this.timeline.push({ type: "system", text: colors.red(`Lỗi gửi yêu cầu: ${e.message}`) });
      this.requestRender();
    }
  }

  private async handleSlashCommand(command: string): Promise<void> {
    const parts = command.split(/\s+/);
    const cmd = parts[0].toLowerCase();
    const arg = parts.slice(1).join(" ").trim();

    switch (cmd) {
      case "/help":
        this.appendSystemLog(colors.bold(colors.cyan("Danh sách lệnh TUI:")));
        this.appendSystemLog(`  ${colors.yellow("/help")}                 - Xem hướng dẫn này`);
        this.appendSystemLog(`  ${colors.yellow("/clear")}                - Xóa màn hình tin nhắn`);
        this.appendSystemLog(`  ${colors.yellow("/interrupt")}            - Dừng lượt xử lý hiện tại của model`);
        this.appendSystemLog(`  ${colors.yellow("/model <tên>")}          - Đổi model (vd: /model default)`);
        this.appendSystemLog(`  ${colors.yellow("/perm <chế_độ>")}        - Đổi quyền (ReadOnly, WorkspaceWrite, DangerFullAccess)`);
        this.appendSystemLog(`  ${colors.yellow("/exit")} hoặc Ctrl+D/Q    - Thoát giao diện TUI`);
        break;

      case "/clear":
        this.timeline = [];
        this.currentThinking = null;
        this.currentStreamingText = "";
        this.renderer.reset();
        break;

      case "/interrupt":
        if (this.currentTurnId && this.sessionId) {
          try {
            await this.client.interruptTurn(this.sessionId, this.currentTurnId);
            this.appendSystemLog(colors.yellow("Đã gửi yêu cầu ngắt lượt xử lý."));
          } catch (e: any) {
            this.appendSystemLog(colors.red(`Lỗi khi ngắt: ${e.message}`));
          }
        } else {
          this.appendSystemLog(colors.dim("Không có lượt xử lý nào đang chạy để ngắt."));
        }
        break;

      case "/model":
        if (!arg) {
          // Mở giao diện interactive ModelSelector kiểu Pi
          try {
            const models = await this.client.listModels();
            if (models.length === 0) {
              this.appendSystemLog(colors.yellow("Không tìm thấy model nào từ server. Dùng /model <tên> để đặt thủ công."));
            } else {
              this.activeModelSelector = new ModelSelector({
                models,
                currentModel: this.activeModel,
                onSelect: async (selectedId: string) => {
                  this.activeModelSelector = null;
                  this.activeModel = selectedId;
                  this.statusBar.update({ modelName: this.activeModel });
                  this.editor.setModelBadge(this.activeModel);
                  this.appendSystemLog(`Đã chuyển sang model: ${colors.cyan(this.activeModel)}`);

                  if (this.sessionId) {
                    try {
                      await this.client.updateSession(this.sessionId, { model: this.activeModel });
                    } catch (e: any) {
                      this.appendSystemLog(colors.red(`Không thể đổi model session: ${e.message}`));
                    }
                  }
                  this.requestRender();
                },
                onCancel: () => {
                  this.activeModelSelector = null;
                  this.requestRender();
                },
              });
            }
          } catch (e: any) {
            this.appendSystemLog(colors.red(`Không thể tải danh sách model: ${e.message}`));
          }
        } else {
          this.activeModel = arg;
          this.statusBar.update({ modelName: this.activeModel });
          this.editor.setModelBadge(this.activeModel);
          this.appendSystemLog(`Đã đổi model sang: ${colors.cyan(this.activeModel)}`);
          if (this.sessionId) {
            try {
              await this.client.updateSession(this.sessionId, { model: this.activeModel });
            } catch (e: any) {
              this.appendSystemLog(colors.red(`Không thể đổi model session: ${e.message}`));
            }
          }
        }
        break;

      case "/perm":
        if (!arg) {
          this.appendSystemLog(`Permission mode hiện tại: ${colors.yellow(this.permissionMode)}`);
        } else {
          this.permissionMode = arg;
          this.statusBar.update({ permissionMode: this.permissionMode });
          if (this.sessionId) {
            try {
              await this.client.updateSession(this.sessionId, { permissionMode: this.permissionMode });
              this.appendSystemLog(`Đã cập nhật permission mode: ${colors.yellow(this.permissionMode)}`);
            } catch (e: any) {
              this.appendSystemLog(colors.red(`Lỗi cập nhật permission mode: ${e.message}`));
            }
          }
        }
        break;

      case "/exit":
        this.stop();
        return;

      default:
        this.appendSystemLog(colors.red(`Lệnh không nhận diện: ${cmd}. Nhập /help để xem trợ giúp.`));
    }

    this.requestRender();
  }

  private appendSystemLog(text: string): void {
    const lines = text.split("\n");
    for (const l of lines) {
      this.timeline.push({ type: "system", text: l });
    }
  }

  private renderUserBubble(text: string, width: number): string[] {
    const safeWidth = Math.max(20, width);
    const bg = "\x1b[48;2;28;37;54m"; // Nền xanh thẫm tối (dark blue/slate) giống Pi/Claude
    const fg = "\x1b[38;2;220;228;240m"; // Chữ sáng
    const reset = "\x1b[0m";

    const lines: string[] = [];
    const textLines = text.split("\n");

    lines.push(""); // Dòng trống ngăn cách phía trên
    for (const rawLine of textLines) {
      const content = ` ${rawLine} `;
      const vLen = visualWidth(content);
      const padLen = Math.max(0, safeWidth - vLen);
      lines.push(`${bg}${fg}${content}${" ".repeat(padLen)}${reset}`);
    }
    lines.push(""); // Dòng trống ngăn cách trước phản hồi AI
    return lines;
  }

  private appendLog(text: string): void {
    this.appendSystemLog(text);
  }

  private splitLines(text: string): string[] {
    return text.split("\n");
  }

  private requestRender(): void {
    const width = this.terminal.columns;
    const lines: string[] = [];
    this.thinkLineRanges = [];

    // 1. Header & StatusBar
    lines.push(...this.statusBar.render(width));

    // 2. Timeline theo đúng thứ tự xuất hiện thực tế (user -> think -> mess -> tool -> think ...)
    for (const item of this.timeline) {
      if (item.type === "user") {
        lines.push(...this.renderUserBubble(item.text, width));
      } else if (item.type === "think") {
        const start = lines.length;
        const rendered = item.block.render(width);
        lines.push(...rendered);
        this.thinkLineRanges.push({ start, end: start + rendered.length, block: item.block });
      } else if (item.type === "text") {
        // Text có thể nhiều dòng: phải tách thành từng dòng, nếu không
        // DifferentialRenderer sẽ lệch con trỏ và in lặp nội dung mỗi frame.
        lines.push(...new StreamingMarkdown(item.text).render(width));
      } else if (item.type === "tool") {
        const icon = item.status === "running" ? colors.yellow("●") : item.status === "done" ? colors.green("✔") : colors.red("✖");
        lines.push(`${icon} ${colors.bold(item.tool)} ${colors.dim(`(${item.status})`)}`);
      } else if (item.type === "system") {
        lines.push(...this.splitLines(item.text));
      }
    }

    // 3. Streaming text hiện tại (chưa kết thúc lượt)
    if (this.currentStreamingText.length > 0) {
      lines.push(...new StreamingMarkdown(this.currentStreamingText).render(width));
    }

    // 4. Spinner loader
    if (this.spinner.isActive()) {
      lines.push(...this.spinner.render(width));
    }

    // 5. Modal Approval (nếu đang chờ phê duyệt quyền)
    if (this.activeApproval) {
      lines.push("");
      lines.push(...this.activeApproval.render(width));
      lines.push("");
    }

    // 6. Model Selector (nếu đang mở menu chọn model)
    if (this.activeModelSelector) {
      lines.push("");
      lines.push(...this.activeModelSelector.render(width));
      lines.push("");
    }

    // 7. Slash Menu popup (nếu đang gõ /)
    if (this.slashMenu) {
      lines.push(...this.slashMenu.render(width));
    }

    // 8. Editor input
    lines.push(...this.editor.render(width));

    this.renderer.renderLines(lines);
  }

  stop(): void {
    if (this.spinnerTimer) {
      clearInterval(this.spinnerTimer);
      this.spinnerTimer = null;
    }
    if (this.unsubscribeEvents) {
      this.unsubscribeEvents();
      this.unsubscribeEvents = null;
    }
    this.terminal.stop();
    console.log(colors.green("\nĐã thoát Ananke TUI. Hẹn gặp lại!"));
    process.exit(0);
  }
}

async function main() {
  const serverUrl = process.env.ANANKE_SERVER_URL || "http://127.0.0.1:9876";
  const app = new AnankeTuiApp(serverUrl);
  await app.start();
}

main().catch((err) => {
  console.error("Lỗi khởi động TUI:", err);
  process.exit(1);
});
