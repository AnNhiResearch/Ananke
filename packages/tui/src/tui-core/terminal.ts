import * as fs from "node:fs";

export interface Terminal {
  // Bắt đầu chế độ raw mode, lắng nghe phím và resize
  start(onInput: (data: string) => void, onResize: () => void): void;
  // Dừng chế độ raw mode và khôi phục trạng thái terminal
  stop(): void;
  // Ghi chuỗi ra stdout
  write(data: string): void;
  // Kích thước terminal
  get columns(): number;
  get rows(): number;
  // Thao tác con trỏ
  moveBy(lines: number): void;
  hideCursor(): void;
  showCursor(): void;
  // Xóa màn hình / dòng
  clearLine(): void;
  clearFromCursor(): void;
  clearScreen(): void;
  // Tiêu đề cửa sổ
  setTitle(title: string): void;
}

/**
 * Terminal thật bọc quanh process.stdin và process.stdout
 * Hỗ trợ raw mode, đo kích thước, resize event (SIGWINCH), zero-flicker escapes
 */
export class ProcessTerminal implements Terminal {
  private wasRaw = false;
  private inputHandler?: (data: string) => void;
  private resizeHandler?: () => void;
  private stdinDataHandler?: (data: Buffer | string) => void;

  get columns(): number {
    return process.stdout.columns || 80;
  }

  get rows(): number {
    return process.stdout.rows || 24;
  }

  start(onInput: (data: string) => void, onResize: () => void): void {
    this.inputHandler = onInput;
    this.resizeHandler = onResize;

    // Lưu trạng thái trước và bật raw mode
    this.wasRaw = process.stdin.isRaw || false;
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdin.setEncoding("utf8");
    process.stdin.resume();

    // Bật chế độ bracketed paste: bọc paste trong \x1b[200~ ... \x1b[201~
    process.stdout.write("\x1b[?2004h");

    // Bật báo cáo sự kiện chuột SGR 1006 + 1000 cho các terminal hỗ trợ click chuột
    process.stdout.write("\x1b[?1000h\x1b[?1006h");

    // Ẩn con trỏ phần cứng mặc định của terminal
    this.hideCursor();

    // Lắng nghe sự kiện thay đổi kích thước terminal (SIGWINCH trên Unix / resize trên Windows)
    process.stdout.on("resize", this.resizeHandler);

    // Lắng nghe dữ liệu nhập từ stdin
    this.stdinDataHandler = (data: Buffer | string) => {
      const str = typeof data === "string" ? data : data.toString("utf8");
      if (this.inputHandler) {
        this.inputHandler(str);
      }
    };
    process.stdin.on("data", this.stdinDataHandler);

    // Xử lý dọn dẹp an toàn khi tiến trình thoát đột ngột (SIGINT, SIGTERM, exit)
    const cleanup = () => {
      this.stop();
    };
    process.once("exit", cleanup);
  }

  stop(): void {
    // Tắt bracketed paste mode và chuột
    try {
      process.stdout.write("\x1b[?1006l\x1b[?1000l\x1b[?2004l");
      this.showCursor();
    } catch {
      // Bỏ qua nếu stdout đã đóng
    }

    // Gỡ các listener
    if (this.stdinDataHandler) {
      process.stdin.removeListener("data", this.stdinDataHandler);
      this.stdinDataHandler = undefined;
    }
    this.inputHandler = undefined;

    if (this.resizeHandler) {
      process.stdout.removeListener("resize", this.resizeHandler);
      this.resizeHandler = undefined;
    }

    // Tạm dừng stdin và khôi phục trạng thái ban đầu
    try {
      process.stdin.pause();
      if (process.stdin.setRawMode) {
        process.stdin.setRawMode(this.wasRaw);
      }
    } catch {
      // Bỏ qua
    }
  }

  write(data: string): void {
    process.stdout.write(data);
  }

  moveBy(lines: number): void {
    if (lines < 0) {
      process.stdout.write(`\x1b[${-lines}A`); // Lên N dòng
    } else if (lines > 0) {
      process.stdout.write(`\x1b[${lines}B`); // Xuống N dòng
    }
  }

  hideCursor(): void {
    process.stdout.write("\x1b[?25l");
  }

  showCursor(): void {
    process.stdout.write("\x1b[?25h");
  }

  clearLine(): void {
    process.stdout.write("\x1b[2K\r");
  }

  clearFromCursor(): void {
    process.stdout.write("\x1b[J");
  }

  clearScreen(): void {
    process.stdout.write("\x1b[2J\x1b[H");
  }

  setTitle(title: string): void {
    process.stdout.write(`\x1b]0;${title}\x07`);
  }
}
