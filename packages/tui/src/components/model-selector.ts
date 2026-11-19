import { Component } from "../tui-core/differential.js";
import { parseKey } from "../tui-core/keys.js";
import { colors, visualWidth } from "../tui-core/colors.js";
import { ModelItem } from "../client.js";

export interface ModelSelectorOptions {
  models: ModelItem[];
  currentModel: string;
  onSelect: (modelId: string) => void;
  onCancel: () => void;
}

/**
 * ModelSelector Component:
 * - Hiển thị danh sách model từ server theo phong cách Pi TUI.
 * - Cho phép lọc/search nhanh.
 * - Điều hướng Up/Down, Enter để chọn, Escape/Ctrl+C để hủy.
 */
export class ModelSelector implements Component {
  private models: ModelItem[];
  private currentModel: string;
  private onSelect: (modelId: string) => void;
  private onCancel: () => void;

  private selectedIndex = 0;
  private filterQuery = "";
  private pageSize = 8;
  private scrollOffset = 0;

  constructor(options: ModelSelectorOptions) {
    this.models = options.models.length > 0 ? options.models : [
      { id: "default", name: "default", provider: "system" }
    ];
    this.currentModel = options.currentModel;
    this.onSelect = options.onSelect;
    this.onCancel = options.onCancel;

    // Đặt con trỏ vào model hiện tại nếu có
    const idx = this.models.findIndex((m) => m.id === this.currentModel);
    if (idx >= 0) {
      this.selectedIndex = idx;
    }
  }

  private getFiltered(): ModelItem[] {
    if (!this.filterQuery.trim()) return this.models;
    const q = this.filterQuery.toLowerCase();
    return this.models.filter(
      (m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q)
    );
  }

  handleInput(data: string): boolean {
    const key = parseKey(data);
    const filtered = this.getFiltered();

    if (key.name === "escape") {
      this.onCancel();
      return true;
    }

    if (key.name === "enter") {
      if (filtered.length > 0 && this.selectedIndex < filtered.length) {
        this.onSelect(filtered[this.selectedIndex].id);
      } else {
        this.onCancel();
      }
      return true;
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

    if (key.name === "backspace") {
      if (this.filterQuery.length > 0) {
        this.filterQuery = this.filterQuery.slice(0, -1);
        this.selectedIndex = 0;
        this.scrollOffset = 0;
      }
      return true;
    }

    // Gõ ký tự tìm kiếm lọc model
    if (!key.ctrl && !key.alt && key.name.length === 1) {
      this.filterQuery += key.name;
      this.selectedIndex = 0;
      this.scrollOffset = 0;
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
    const safeWidth = Math.max(40, width);
    const boxWidth = Math.min(safeWidth - 4, 76);
    const filtered = this.getFiltered();

    const lines: string[] = [];

    // Header notice
    lines.push(colors.yellow("Only showing models from configured providers."));
    lines.push(`> ${this.filterQuery}${colors.inverse(" ")}`);
    lines.push("");

    if (filtered.length === 0) {
      lines.push(colors.dim("  (Không tìm thấy model nào phù hợp)"));
    } else {
      const visible = filtered.slice(this.scrollOffset, this.scrollOffset + this.pageSize);
      for (let i = 0; i < visible.length; i++) {
        const item = visible[i];
        const actualIdx = this.scrollOffset + i;
        const isSelected = actualIdx === this.selectedIndex;
        const isCurrent = item.id === this.currentModel;

        const cursor = isSelected ? colors.magenta("→ ") : "  ";
        const check = isCurrent ? colors.magenta("✓ ") : "  ";
        const modelName = isSelected ? colors.bold(colors.magenta(item.id)) : item.id;
        const providerTag = colors.dim(`[${item.provider}]`);

        lines.push(`${cursor}${check}${modelName} ${providerTag}`);
      }

      // Chỉ số phân trang
      const countStr = `(${this.selectedIndex + 1}/${filtered.length})`;
      lines.push(colors.dim(countStr));
    }

    lines.push("");
    const selectedItem = filtered[this.selectedIndex];
    if (selectedItem) {
      lines.push(`Model Name: ${colors.bold(selectedItem.name || selectedItem.id)}`);
      if (selectedItem.description) {
        lines.push(colors.dim(`Description: ${selectedItem.description}`));
      }
    }

    lines.push("");
    lines.push(colors.green("Model catalogs refreshed."));
    lines.push("");
    lines.push(colors.dim("Enter to select · Escape/Ctrl+C to cancel"));

    return lines;
  }
}
