import { Component } from "../tui-core/differential.js";
import { parseKey } from "../tui-core/keys.js";
import { colors, visualWidth } from "../tui-core/colors.js";

export interface SlashCommandItem {
  name: string;
  description: string;
}

export interface SlashMenuOptions {
  commands: SlashCommandItem[];
  filter: string;
  onSelect: (commandName: string) => void;
  onClose: () => void;
}

/**
 * SlashMenu Component:
 * - Hiển thị menu gợi ý lệnh khi người dùng gõ `/` (tương tự Pi TUI).
 * - Lọc danh sách theo chữ gõ sau `/`.
 * - Phím Up/Down điều hướng con trỏ `→`.
 * - Phím Tab/Enter hoàn thành lệnh.
 */
export class SlashMenu implements Component {
  private commands: SlashCommandItem[];
  private filter = "";
  private selectedIndex = 0;
  private pageSize = 5;
  private scrollOffset = 0;
  private onSelect: (cmd: string) => void;
  private onClose: () => void;

  constructor(options: SlashMenuOptions) {
    this.commands = options.commands;
    this.filter = options.filter.replace(/^\//, "").toLowerCase();
    this.onSelect = options.onSelect;
    this.onClose = options.onClose;
  }

  setFilter(filter: string): void {
    this.filter = filter.replace(/^\//, "").toLowerCase();
    this.selectedIndex = 0;
    this.scrollOffset = 0;
  }

  private getFiltered(): SlashCommandItem[] {
    if (!this.filter) return this.commands;
    return this.commands.filter(
      (c) => c.name.toLowerCase().includes(this.filter) || c.description.toLowerCase().includes(this.filter)
    );
  }

  handleInput(data: string): boolean {
    const key = parseKey(data);
    const filtered = this.getFiltered();

    if (key.name === "escape") {
      this.onClose();
      return true;
    }

    if (key.name === "enter" || key.name === "tab") {
      if (filtered.length > 0 && this.selectedIndex < filtered.length) {
        this.onSelect("/" + filtered[this.selectedIndex].name);
        return true;
      }
    }

    if (key.name === "up") {
      if (filtered.length > 0) {
        this.selectedIndex = (this.selectedIndex - 1 + filtered.length) % filtered.length;
        this.adjustScroll(filtered.length);
      }
      return true;
    }

    if (key.name === "down") {
      if (filtered.length > 0) {
        this.selectedIndex = (this.selectedIndex + 1) % filtered.length;
        this.adjustScroll(filtered.length);
      }
      return true;
    }

    return false;
  }

  private adjustScroll(total: number): void {
    if (this.selectedIndex < this.scrollOffset) {
      this.scrollOffset = this.selectedIndex;
    } else if (this.selectedIndex >= this.scrollOffset + this.pageSize) {
      this.scrollOffset = this.selectedIndex - this.pageSize + 1;
    }
    this.scrollOffset = Math.max(0, Math.min(this.scrollOffset, Math.max(0, total - this.pageSize)));
  }

  render(width: number): string[] {
    const safeWidth = Math.max(20, width);
    const filtered = this.getFiltered();
    if (filtered.length === 0) return [];

    const lines: string[] = [];
    const visible = filtered.slice(this.scrollOffset, this.scrollOffset + this.pageSize);

    // Tính cột lệnh dài nhất để căn lề đều
    const maxCmdLen = 14;

    for (let i = 0; i < visible.length; i++) {
      const item = visible[i];
      const actualIdx = this.scrollOffset + i;
      const isSelected = actualIdx === this.selectedIndex;

      const cursor = isSelected ? colors.magenta("→ ") : "  ";
      const cmdPadded = item.name.padEnd(maxCmdLen);
      const cmdText = isSelected ? colors.bold(colors.magenta(cmdPadded)) : colors.bold(cmdPadded);
      const descText = isSelected ? colors.magenta(item.description) : colors.dim(item.description);

      const content = `${cursor}${cmdText}  ${descText}`;
      const vLen = 2 + maxCmdLen + 2 + item.description.length;
      const pad = " ".repeat(Math.max(0, safeWidth - vLen));
      lines.push(`${content}${pad}`);
    }

    // Hiển thị chỉ số trang (5/26) kiểu Pi
    const pageIndexStr = `  (${this.selectedIndex + 1}/${filtered.length})`;
    lines.push(colors.dim(pageIndexStr) + " ".repeat(Math.max(0, safeWidth - pageIndexStr.length)));

    return lines;
  }
}
