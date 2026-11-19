import { Component } from "../tui-core/differential.js";
import { colors, visualWidth } from "../tui-core/colors.js";

export interface ThinkBlockOptions {
  content?: string;
  expanded?: boolean;
}

/**
 * ThinkBlock:
 * Khối hiển thị suy nghĩ (thinking/reasoning) của model theo phong cách Pi TUI.
 * Mặc định ở trạng thái thu gọn `Thinking... (bấm Ctrl+T để xem/ẩn)`.
 * Khi mở rộng sẽ in toàn bộ nội dung reasoning với chữ nghiêng/mờ (italic/dim).
 */
export class ThinkBlock implements Component {
  private content = "";
  private isExpanded = false;

  constructor(options: ThinkBlockOptions = {}) {
    this.content = options.content || "";
    this.isExpanded = options.expanded ?? false;
  }

  append(delta: string): void {
    this.content += delta;
  }

  setContent(text: string): void {
    this.content = text;
  }

  getContent(): string {
    return this.content;
  }

  toggle(): void {
    this.isExpanded = !this.isExpanded;
  }

  setExpanded(expanded: boolean): void {
    this.isExpanded = expanded;
  }

  expanded(): boolean {
    return this.isExpanded;
  }

  clear(): void {
    this.content = "";
    this.isExpanded = false;
  }

  render(width: number): string[] {
    if (!this.content.trim()) return [];
    const safeWidth = Math.max(20, width);
    const lines: string[] = [];

    if (!this.isExpanded) {
      // Header thu gọn: Thinking... (Click hoặc Ctrl+T để xem)
      const label = "Thinking...";
      const hint = " (Click hoặc Ctrl+T để xem)";
      const full = colors.italic(colors.dim(label)) + colors.dim(hint);
      const vLen = visualWidth(label) + visualWidth(hint);
      lines.push(full + " ".repeat(Math.max(0, safeWidth - vLen)));
    } else {
      // Header mở rộng
      const header = "Thinking (Click hoặc Ctrl+T để thu gọn):";
      lines.push(colors.italic(colors.cyan(header)));
      
      const contentLines = this.content.split("\n");
      for (const line of contentLines) {
        const text = colors.italic(colors.dim(line));
        const vLen = visualWidth(line);
        lines.push(text + " ".repeat(Math.max(0, safeWidth - vLen)));
      }
      lines.push(colors.gray("─".repeat(safeWidth)));
    }

    return lines;
  }
}
