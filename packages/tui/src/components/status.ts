import { Component } from "../tui-core/differential.js";
import { colors, visualWidth } from "../tui-core/colors.js";

/**
 * Spinner / Loader Component:
 * - Chỉ báo trạng thái model suy nghĩ / đang thực thi tool kèm spinner động và elapsed time.
 */
export class Spinner implements Component {
  private static readonly FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
  private frameIndex = 0;
  private message: string;
  private startTime: number;
  private active = false;

  constructor(initialMessage = "Đang xử lý...") {
    this.message = initialMessage;
    this.startTime = Date.now();
  }

  start(message?: string): void {
    if (message) this.message = message;
    this.startTime = Date.now();
    this.active = true;
    this.frameIndex = 0;
  }

  stop(): void {
    this.active = false;
  }

  tick(): void {
    if (this.active) {
      this.frameIndex = (this.frameIndex + 1) % Spinner.FRAMES.length;
    }
  }

  setMessage(message: string): void {
    this.message = message;
  }

  isActive(): boolean {
    return this.active;
  }

  render(_width: number): string[] {
    if (!this.active) return [];
    const frame = Spinner.FRAMES[this.frameIndex];
    const elapsedSec = ((Date.now() - this.startTime) / 1000).toFixed(1);
    return [
      `  ${colors.cyan(frame)} ${colors.bold(this.message)} ${colors.dim(`(${elapsedSec}s)`)}`,
    ];
  }
}

export interface StatusBarProps {
  sessionTitle?: string;
  modelName: string;
  sandboxMode: "none" | "restricted-token" | "full";
  permissionMode: string;
}

/**
 * StatusBar & Header Component:
 * - Hiển thị model hiện tại, chế độ sandbox (cảnh báo đỏ nếu none), và permission mode.
 */
export class StatusBar implements Component {
  private props: StatusBarProps;

  constructor(props: StatusBarProps) {
    this.props = props;
  }

  update(props: Partial<StatusBarProps>): void {
    this.props = { ...this.props, ...props };
  }

  render(width: number): string[] {
    const safeWidth = Math.max(20, width);
    const sessionLabel = colors.bold(` Ananke TUI `);
    const modelTag = colors.cyan(`[Model: ${this.props.modelName}]`);
    const permTag = colors.yellow(`[Perm: ${this.props.permissionMode}]`);

    let sandboxTag: string;
    if (this.props.sandboxMode === "none") {
      sandboxTag = colors.bgRed(colors.white(" SANDBOX: NONE (INSECURE) "));
    } else {
      sandboxTag = colors.green(`[Sandbox: ${this.props.sandboxMode}]`);
    }

    const leftPart = `${sessionLabel} ${modelTag}`;
    const rightPart = `${permTag} ${sandboxTag} `;

    const leftWidth = visualWidth(leftPart);
    const rightWidth = visualWidth(rightPart);
    const spaceCount = Math.max(1, safeWidth - leftWidth - rightWidth);
    const line = leftPart + " ".repeat(spaceCount) + rightPart;

    return [
      colors.bgBlue(colors.white(line)),
      colors.gray("─".repeat(safeWidth)),
    ];
  }
}
