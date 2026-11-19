import { Component } from "../tui-core/differential.js";
import { parseKey, KeyEvent } from "../tui-core/keys.js";
import { colors } from "../tui-core/colors.js";

/**
 * Text có thể chèn vào editor: không rỗng, không chứa escape sequence
 * và không chứa ký tự điều khiển (Enter/Tab/Backspace... đã xử lý riêng).
 */
function isPrintableText(text: string): boolean {
  if (!text) return false;
  for (const ch of text) {
    const cp = ch.codePointAt(0);
    if (cp === undefined) continue;
    if (cp < 32 || cp === 0x7f) return false;
  }
  return true;
}

export interface EditorOptions {
  placeholder?: string;
  modelBadge?: string;
  onSubmit?: (text: string) => void;
  onChange?: (text: string) => void;
}

/**
 * Editor Component:
 * - Hỗ trợ nhập liệu nhiều dòng.
 * - Điều hướng con trỏ (Trái, Phải, Lên, Xuống, Home, End).
 * - Enter: gửi nội dung (nếu không nhấn Shift).
 * - Shift+Enter hoặc Alt+Enter: ngắt dòng mới.
 * - Lịch sử lệnh (Up/Down khi ở dòng đầu/cuối).
 * - Render đường viền và vị trí con trỏ ANSI.
 */
export class Editor implements Component {
  private lines: string[] = [""];
  private cursorRow = 0;
  private cursorCol = 0;
  private history: string[] = [];
  private historyIndex = -1;
  private historyDraft = "";
  private placeholder: string;
  private modelBadge: string = "";
  private onSubmit?: (text: string) => void;
  private onChange?: (text: string) => void;

  constructor(options: EditorOptions = {}) {
    this.placeholder = options.placeholder || "Nhập tin nhắn hoặc / lệnh... (Shift+Enter xuống dòng, Enter gửi)";
    this.modelBadge = options.modelBadge || "";
    this.onSubmit = options.onSubmit;
    this.onChange = options.onChange;
  }

  setModelBadge(badge: string): void {
    this.modelBadge = badge;
  }

  getText(): string {
    return this.lines.join("\n");
  }

  setText(text: string): void {
    const split = text.split("\n");
    this.lines = split.length ? split : [""];
    this.cursorRow = this.lines.length - 1;
    this.cursorCol = this.lines[this.cursorRow].length;
    this.notifyChange();
  }

  clear(): void {
    this.lines = [""];
    this.cursorRow = 0;
    this.cursorCol = 0;
    this.historyIndex = -1;
    this.historyDraft = "";
    this.notifyChange();
  }

  private notifyChange(): void {
    if (this.onChange) {
      this.onChange(this.getText());
    }
  }

  handleInput(data: string): boolean {
    const key = parseKey(data);

    // 1. Bracketed paste block
    if (key.isPaste) {
      const pasteLines = key.name.split(/\r?\n/);
      for (let i = 0; i < pasteLines.length; i++) {
        if (i > 0) this.insertNewline();
        this.insertText(pasteLines[i]);
      }
      this.notifyChange();
      return true;
    }

    // 2. Submit / Newline
    if (key.name === "enter") {
      if (key.shift || key.alt) {
        this.insertNewline();
        this.notifyChange();
        return true;
      }
      const fullText = this.getText().trim();
      if (fullText.length > 0) {
        this.history.push(fullText);
      }
      this.clear();
      if (this.onSubmit) {
        this.onSubmit(fullText);
      }
      return true;
    }

    // 3. Backspace
    if (key.name === "backspace") {
      this.handleBackspace();
      this.notifyChange();
      return true;
    }

    // 4. Con trỏ trái/phải
    if (key.name === "left") {
      this.moveCursorLeft();
      return true;
    }
    if (key.name === "right") {
      this.moveCursorRight();
      return true;
    }

    // 5. Con trỏ lên/xuống (kèm history navigation)
    if (key.name === "up") {
      if (this.cursorRow > 0) {
        this.cursorRow--;
        this.cursorCol = Math.min(this.cursorCol, this.lines[this.cursorRow].length);
      } else {
        this.navigateHistory(-1);
      }
      return true;
    }
    if (key.name === "down") {
      if (this.cursorRow < this.lines.length - 1) {
        this.cursorRow++;
        this.cursorCol = Math.min(this.cursorCol, this.lines[this.cursorRow].length);
      } else {
        this.navigateHistory(1);
      }
      return true;
    }

    // 6. Home / End
    if (key.name === "home") {
      this.cursorCol = 0;
      return true;
    }
    if (key.name === "end") {
      this.cursorCol = this.lines[this.cursorRow].length;
      return true;
    }

    // 7. Space
    if (key.name === "space") {
      this.insertText(" ");
      this.notifyChange();
      return true;
    }

    // 8. Nhập ký tự thông thường
    // Dùng key.raw (không dùng key.name đã lower-case) và cho phép NHIỀU ký tự
    // trong một chunk: IME (UniKey/EVKey) gửi cả cụm như "chào", "Đ" trong một
    // lần đọc stdin, nếu chỉ nhận 1 ký tự thì cả cụm bị nuốt mất.
    if (!key.ctrl && !key.alt && isPrintableText(key.raw)) {
      this.insertText(key.raw);
      this.notifyChange();
      return true;
    }

    return false;
  }

  private insertText(text: string): void {
    const char = text;
    const curLine = this.lines[this.cursorRow];
    this.lines[this.cursorRow] = curLine.slice(0, this.cursorCol) + char + curLine.slice(this.cursorCol);
    this.cursorCol += char.length;
  }

  private insertNewline(): void {
    const curLine = this.lines[this.cursorRow];
    const before = curLine.slice(0, this.cursorCol);
    const after = curLine.slice(this.cursorCol);
    this.lines[this.cursorRow] = before;
    this.lines.splice(this.cursorRow + 1, 0, after);
    this.cursorRow++;
    this.cursorCol = 0;
  }

  private handleBackspace(): void {
    if (this.cursorCol > 0) {
      const curLine = this.lines[this.cursorRow];
      this.lines[this.cursorRow] = curLine.slice(0, this.cursorCol - 1) + curLine.slice(this.cursorCol);
      this.cursorCol--;
    } else if (this.cursorRow > 0) {
      const prevLine = this.lines[this.cursorRow - 1];
      const curLine = this.lines[this.cursorRow];
      this.cursorCol = prevLine.length;
      this.lines[this.cursorRow - 1] = prevLine + curLine;
      this.lines.splice(this.cursorRow, 1);
      this.cursorRow--;
    }
  }

  private moveCursorLeft(): void {
    if (this.cursorCol > 0) {
      this.cursorCol--;
    } else if (this.cursorRow > 0) {
      this.cursorRow--;
      this.cursorCol = this.lines[this.cursorRow].length;
    }
  }

  private moveCursorRight(): void {
    if (this.cursorCol < this.lines[this.cursorRow].length) {
      this.cursorCol++;
    } else if (this.cursorRow < this.lines.length - 1) {
      this.cursorRow++;
      this.cursorCol = 0;
    }
  }

  private navigateHistory(direction: number): void {
    if (this.history.length === 0) return;
    if (this.historyIndex === -1 && direction === -1) {
      this.historyDraft = this.getText();
      this.historyIndex = this.history.length - 1;
      this.setText(this.history[this.historyIndex]);
    } else if (this.historyIndex >= 0) {
      const nextIndex = this.historyIndex + (direction === -1 ? -1 : 1);
      if (nextIndex >= 0 && nextIndex < this.history.length) {
        this.historyIndex = nextIndex;
        this.setText(this.history[this.historyIndex]);
      } else if (nextIndex >= this.history.length) {
        this.historyIndex = -1;
        this.setText(this.historyDraft);
      }
    }
  }

  render(width: number): string[] {
    const safeWidth = Math.max(10, width);
    const border = colors.gray("─".repeat(safeWidth));
    const result: string[] = [border];

    const isBlank = this.lines.length === 1 && this.lines[0].length === 0;

    if (isBlank) {
      const pad = " ".repeat(Math.max(0, safeWidth - 2 - this.placeholder.length));
      result.push(colors.gray(`  ${this.placeholder}${pad}`));
    } else {
      for (let r = 0; r < this.lines.length; r++) {
        const line = this.lines[r];
        if (r === this.cursorRow) {
          const before = line.slice(0, this.cursorCol);
          const cursorChar = line.slice(this.cursorCol, this.cursorCol + 1) || " ";
          const after = line.slice(this.cursorCol + 1);
          const lineContent = `  ${before}${colors.inverse(cursorChar)}${after}`;
          const visibleLen = 2 + line.length + (line.length <= this.cursorCol ? 1 : 0);
          const pad = " ".repeat(Math.max(0, safeWidth - visibleLen));
          result.push(`${lineContent}${pad}`);
        } else {
          const pad = " ".repeat(Math.max(0, safeWidth - 2 - line.length));
          result.push(`  ${line}${pad}`);
        }
      }
    }

    result.push(border);

    // Dòng chân editor: hiển thị tên model ở góc phải (như Pi TUI)
    if (this.modelBadge) {
      const badgeText = colors.dim(this.modelBadge);
      const badgeLen = this.modelBadge.length;
      const leftPad = " ".repeat(Math.max(0, safeWidth - badgeLen));
      result.push(`${leftPad}${badgeText}`);
    }

    return result;
  }
}
