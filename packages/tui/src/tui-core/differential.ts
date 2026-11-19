import { type Terminal } from "./terminal.js";
import { wrapAnsi } from "./colors.js";

/**
 * Interface cho component có thể render ra mảng các dòng chuỗi ANSI.
 */
export interface Component {
  render(width: number): string[];
}

/**
 * Tùy chọn cấu hình cho DifferentialRenderer.
 */
export interface DifferentialRendererOptions {
  terminal: Terminal;
  clearOnShrink?: boolean;
}

/**
 * DifferentialRenderer chịu trách nhiệm so sánh buffer màn hình giữa các frame (differential rendering),
 * chỉ gửi các byte ANSI thay đổi tới terminal bọc trong chuỗi Synchronized Output (CSI ? 2026 h / l)
 * để loại bỏ hoàn toàn hiện tượng nhấp nháy (flickering) và bảo toàn thanh cuộn scrollback.
 */
export class DifferentialRenderer {
  private terminal: Terminal;
  private previousLines: string[] = [];
  private previousWidth = 0;
  private previousHeight = 0;
  private hardwareCursorRow = 0;
  private maxLinesRendered = 0;
  private previousViewportTop = 0;
  private clearOnShrink: boolean;

  constructor(options: DifferentialRendererOptions) {
    this.terminal = options.terminal;
    this.clearOnShrink = options.clearOnShrink ?? true;
  }

  /**
   * Đặt lại trạng thái render khi thay đổi kích thước hoặc xóa toàn bộ màn hình.
   */
  reset(): void {
    this.previousLines = [];
    this.previousWidth = 0;
    this.previousHeight = 0;
    this.hardwareCursorRow = 0;
    this.maxLinesRendered = 0;
    this.previousViewportTop = 0;
  }

  /**
   * Thực hiện differential render danh sách dòng văn bản mới.
   */
  renderLines(newLines: string[]): void {
    const width = this.terminal.columns;
    const height = this.terminal.rows;

    // Bất biến của renderer: mỗi phần tử = ĐÚNG một hàng vật lý.
    // Tách newline sót lại và tự word-wrap mọi dòng dài hơn bề rộng terminal,
    // nếu không terminal tự wrap ngầm và hardwareCursorRow sẽ lệch dần
    // (hậu quả: in lặp nội dung cũ mỗi frame).
    const normalized: string[] = [];
    for (const entry of newLines) {
      for (const part of entry.split("\n")) {
        normalized.push(...wrapAnsi(part, width));
      }
    }
    newLines = normalized;

    const widthChanged = this.previousWidth !== 0 && this.previousWidth !== width;
    const heightChanged = this.previousHeight !== 0 && this.previousHeight !== height;

    const previousBufferLength = this.previousHeight > 0
      ? this.previousViewportTop + this.previousHeight
      : height;
    let prevViewportTop = heightChanged
      ? Math.max(0, previousBufferLength - height)
      : this.previousViewportTop;
    let viewportTop = prevViewportTop;
    let hardwareCursorRow = this.hardwareCursorRow;

    const computeLineDiff = (targetRow: number): number => {
      const currentScreenRow = hardwareCursorRow - prevViewportTop;
      const targetScreenRow = targetRow - viewportTop;
      return targetScreenRow - currentScreenRow;
    };

    // Full render helper
    const fullRender = (clear: boolean): void => {
      let buffer = "\x1b[?2026h"; // Bắt đầu synchronized output
      if (clear) {
        buffer += "\x1b[2J\x1b[H\x1b[3J"; // Clear màn hình & scrollback
      }
      for (let i = 0; i < newLines.length; i++) {
        if (i > 0) buffer += "\r\n";
        buffer += newLines[i];
      }
      buffer += "\x1b[?2026l"; // Kết thúc synchronized output
      this.terminal.write(buffer);

      this.hardwareCursorRow = Math.max(0, newLines.length - 1);
      if (clear) {
        this.maxLinesRendered = newLines.length;
      } else {
        this.maxLinesRendered = Math.max(this.maxLinesRendered, newLines.length);
      }
      const bufferLength = Math.max(height, newLines.length);
      this.previousViewportTop = Math.max(0, bufferLength - height);
      this.previousLines = [...newLines];
      this.previousWidth = width;
      this.previousHeight = height;
    };

    // Lần render đầu tiên
    if (this.previousLines.length === 0 && !widthChanged && !heightChanged) {
      fullRender(false);
      return;
    }

    // Chiều rộng đổi buộc redraw toàn bộ do wrap thay đổi
    if (widthChanged || heightChanged) {
      fullRender(true);
      return;
    }

    // Co rút dòng nếu cấu hình clearOnShrink
    if (this.clearOnShrink && newLines.length < this.maxLinesRendered) {
      fullRender(true);
      return;
    }

    // Tìm dòng thay đổi đầu tiên và dòng cuối cùng
    let firstChanged = -1;
    let lastChanged = -1;
    const maxLines = Math.max(newLines.length, this.previousLines.length);

    for (let i = 0; i < maxLines; i++) {
      const oldLine = i < this.previousLines.length ? this.previousLines[i] : "";
      const newLine = i < newLines.length ? newLines[i] : "";
      if (oldLine !== newLine) {
        if (firstChanged === -1) firstChanged = i;
        lastChanged = i;
      }
    }

    const appendedLines = newLines.length > this.previousLines.length;
    if (appendedLines) {
      if (firstChanged === -1) firstChanged = this.previousLines.length;
      lastChanged = newLines.length - 1;
    }

    // Không có thay đổi nào giữa 2 frame
    if (firstChanged === -1) {
      this.previousViewportTop = prevViewportTop;
      this.previousHeight = height;
      return;
    }

    // Nội dung cũ bị xóa bớt dòng ở cuối
    if (firstChanged >= newLines.length) {
      if (this.previousLines.length > newLines.length) {
        let buffer = "\x1b[?2026h";
        const targetRow = Math.max(0, newLines.length - 1);
        const lineDiff = computeLineDiff(targetRow);
        if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`;
        else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`;
        buffer += "\r";

        const extraLines = this.previousLines.length - newLines.length;
        const clearStartOffset = newLines.length === 0 ? 0 : 1;
        if (extraLines > 0 && clearStartOffset > 0) {
          buffer += `\x1b[${clearStartOffset}B`;
        }
        for (let i = 0; i < extraLines; i++) {
          buffer += "\r\x1b[2K";
          if (i < extraLines - 1) buffer += "\x1b[1B";
        }
        const moveBack = Math.max(0, extraLines - 1 + clearStartOffset);
        if (moveBack > 0) buffer += `\x1b[${moveBack}A`;
        buffer += "\x1b[?2026l";
        this.terminal.write(buffer);
        this.hardwareCursorRow = targetRow;
      }
      this.previousLines = [...newLines];
      this.previousWidth = width;
      this.previousHeight = height;
      this.previousViewportTop = prevViewportTop;
      return;
    }

    // Thay đổi nằm ngoài vùng nhìn thấy phía trên -> cần redraw
    if (firstChanged < prevViewportTop) {
      fullRender(true);
      return;
    }

    // Render differential từ dòng thay đổi đầu tiên
    let buffer = "\x1b[?2026h";
    const prevViewportBottom = prevViewportTop + height - 1;
    const appendStart = appendedLines && firstChanged === this.previousLines.length && firstChanged > 0;
    const moveTargetRow = appendStart ? firstChanged - 1 : firstChanged;

    if (moveTargetRow > prevViewportBottom) {
      const currentScreenRow = Math.max(0, Math.min(height - 1, hardwareCursorRow - prevViewportTop));
      const moveToBottom = height - 1 - currentScreenRow;
      if (moveToBottom > 0) buffer += `\x1b[${moveToBottom}B`;
      const scroll = moveTargetRow - prevViewportBottom;
      buffer += "\r\n".repeat(scroll);
      prevViewportTop += scroll;
      viewportTop += scroll;
      hardwareCursorRow = moveTargetRow;
    }

    // Di chuyển con trỏ tới dòng thay đổi đầu tiên
    const lineDiff = computeLineDiff(moveTargetRow);
    if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`;
    else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`;

    if (appendStart) {
      buffer += "\r\n";
      hardwareCursorRow = firstChanged;
    } else {
      buffer += "\r";
      hardwareCursorRow = firstChanged;
    }

    // Ghi từng dòng thay đổi với mã xóa dòng \x1b[2K\r
    const startLine = firstChanged;
    for (let i = startLine; i < newLines.length; i++) {
      if (i > startLine) buffer += "\r\n";
      buffer += "\r\x1b[2K" + newLines[i];
      hardwareCursorRow = i;
    }

    // Xóa các dòng thừa còn lại nếu frame mới ngắn hơn
    if (this.previousLines.length > newLines.length) {
      const extraLines = this.previousLines.length - newLines.length;
      for (let i = 0; i < extraLines; i++) {
        buffer += "\r\n\r\x1b[2K";
      }
      buffer += `\x1b[${extraLines}A`;
    }

    buffer += "\x1b[?2026l"; // Kết thúc synchronized output
    this.terminal.write(buffer);

    this.hardwareCursorRow = Math.max(0, newLines.length - 1);
    this.previousLines = [...newLines];
    this.previousWidth = width;
    this.previousHeight = height;
    this.maxLinesRendered = Math.max(this.maxLinesRendered, newLines.length);
    const bufferLength = Math.max(height, newLines.length);
    this.previousViewportTop = Math.max(0, bufferLength - height);
  }
}
