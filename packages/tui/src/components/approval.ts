import { Component } from "../tui-core/differential.js";
import { parseKey } from "../tui-core/keys.js";
import { colors, visualWidth } from "../tui-core/colors.js";

export interface ApprovalRequest {
  toolCallId: string;
  toolName: string;
  arguments: unknown;
  reason?: string;
}

export type ApprovalDecision = "allow" | "deny" | "always";

export interface ApprovalDialogOptions {
  request: ApprovalRequest;
  onDecision: (decision: ApprovalDecision) => void;
}

/**
 * ApprovalDialog Component:
 * - Hộp thoại modal tương tác nổi bật khi server gửi sự kiện `permission.requested`.
 * - Hỗ trợ các phím lựa chọn:
 *   + [y / a / Enter]: Allow (Cho phép lần này)
 *   + [n / d / Esc]: Deny (Từ chối)
 *   + [w / Shift+A]: Always Allow (Luôn cho phép)
 */
export class ApprovalDialog implements Component {
  private request: ApprovalRequest;
  private onDecision: (decision: ApprovalDecision) => void;
  private selectedIndex = 0; // 0: Allow, 1: Deny, 2: Always Allow
  private options: { label: string; decision: ApprovalDecision }[] = [
    { label: "1. Cho phép (Allow)", decision: "allow" },
    { label: "2. Từ chối (Deny)", decision: "deny" },
    { label: "3. Luôn cho phép (Always)", decision: "always" },
  ];

  constructor(options: ApprovalDialogOptions) {
    this.request = options.request;
    this.onDecision = options.onDecision;
  }

  handleInput(data: string): boolean {
    const key = parseKey(data);

    // Phím tắt nhanh
    if (key.name === "y" || key.name === "a") {
      this.onDecision("allow");
      return true;
    }
    if (key.name === "n" || key.name === "d" || key.name === "escape") {
      this.onDecision("deny");
      return true;
    }
    if (key.name === "w") {
      this.onDecision("always");
      return true;
    }

    // Điều hướng mũi tên Trái / Phải
    if (key.name === "left" || key.name === "up") {
      this.selectedIndex = (this.selectedIndex - 1 + this.options.length) % this.options.length;
      return true;
    }
    if (key.name === "right" || key.name === "down") {
      this.selectedIndex = (this.selectedIndex + 1) % this.options.length;
      return true;
    }

    // Chọn phím số 1, 2, 3
    if (key.name === "1") {
      this.onDecision("allow");
      return true;
    }
    if (key.name === "2") {
      this.onDecision("deny");
      return true;
    }
    if (key.name === "3") {
      this.onDecision("always");
      return true;
    }

    // Enter xác nhận mục đang chọn
    if (key.name === "enter") {
      this.onDecision(this.options[this.selectedIndex].decision);
      return true;
    }

    return false;
  }

  render(width: number): string[] {
    const safeWidth = Math.max(40, width);
    const boxWidth = Math.min(safeWidth - 4, 76);
    const innerWidth = boxWidth - 4;

    const lines: string[] = [];
    const hBorder = "═".repeat(boxWidth - 2);

    lines.push(colors.bold(colors.yellow(`╔${hBorder}╗`)));
    lines.push(colors.bold(colors.yellow(`║  YÊU CẦU CẤP QUYỀN THỰC THI TOOL ${" ".repeat(Math.max(0, boxWidth - 36))}║`)));
    lines.push(colors.bold(colors.yellow(`╠${hBorder}╣`)));

    // Tên tool & Đối số
    const toolLine = `  Tool   : ${colors.bold(colors.cyan(this.request.toolName))}`;
    lines.push(colors.yellow("║") + toolLine + " ".repeat(Math.max(0, innerWidth - visualWidth(toolLine) + 2)) + colors.yellow("║"));

    const argJson = JSON.stringify(this.request.arguments);
    const truncatedArgs = argJson.length > innerWidth - 12 ? argJson.slice(0, innerWidth - 15) + "..." : argJson;
    const argsLine = `  Args   : ${colors.dim(truncatedArgs)}`;
    lines.push(colors.yellow("║") + argsLine + " ".repeat(Math.max(0, innerWidth - visualWidth(argsLine) + 2)) + colors.yellow("║"));

    if (this.request.reason) {
      const reasonLine = `  Lý do  : ${this.request.reason}`;
      lines.push(colors.yellow("║") + reasonLine + " ".repeat(Math.max(0, innerWidth - visualWidth(reasonLine) + 2)) + colors.yellow("║"));
    }

    lines.push(colors.yellow(`║${" ".repeat(boxWidth - 2)}║`));

    // Nút lựa chọn
    let buttons = "  ";
    for (let i = 0; i < this.options.length; i++) {
      const opt = this.options[i];
      if (i === this.selectedIndex) {
        buttons += colors.inverse(colors.bold(` [ ${opt.label} ] `)) + "  ";
      } else {
        buttons += colors.dim(` [ ${opt.label} ] `) + "  ";
      }
    }
    lines.push(colors.yellow("║") + buttons + " ".repeat(Math.max(0, innerWidth - visualWidth(buttons) + 2)) + colors.yellow("║"));
    lines.push(colors.bold(colors.yellow(`╚${hBorder}╝`)));

    return lines;
  }
}
