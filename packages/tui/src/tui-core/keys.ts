/**
 * Danh sách tên phím bổ trợ (Modifiers)
 */
export type ModifierName = "ctrl" | "alt" | "shift" | "super";

/**
 * Interface biểu diễn sự kiện phím sau khi phân tích từ chuỗi raw bytes stdin.
 */
export interface KeyEvent {
  name: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  super: boolean;
  raw: string;
  isPaste?: boolean;
}

/**
 * Map các escape sequences phổ biến từ terminal.
 */
const ESCAPE_SEQUENCES: Record<string, { name: string; shift?: boolean; ctrl?: boolean; alt?: boolean }> = {
  "\x1b[A": { name: "up" },
  "\x1b[B": { name: "down" },
  "\x1b[C": { name: "right" },
  "\x1b[D": { name: "left" },
  "\x1b[H": { name: "home" },
  "\x1b[F": { name: "end" },
  "\x1b[1~": { name: "home" },
  "\x1b[4~": { name: "end" },
  "\x1b[2~": { name: "insert" },
  "\x1b[3~": { name: "delete" },
  "\x1b[5~": { name: "pageUp" },
  "\x1b[6~": { name: "pageDown" },
  "\x1b[Z": { name: "tab", shift: true }, // Shift+Tab
  "\x1bOP": { name: "f1" },
  "\x1bOQ": { name: "f2" },
  "\x1bOR": { name: "f3" },
  "\x1bOS": { name: "f4" },
  "\x1b[15~": { name: "f5" },
  "\x1b[17~": { name: "f6" },
  "\x1b[18~": { name: "f7" },
  "\x1b[19~": { name: "f8" },
  "\x1b[20~": { name: "f9" },
  "\x1b[21~": { name: "f10" },
  "\x1b[23~": { name: "f11" },
  "\x1b[24~": { name: "f12" },

  // CSI with modifiers (ví dụ: \x1b[1;5A -> Ctrl+Up, \x1b[1;2A -> Shift+Up)
  "\x1b[1;5A": { name: "up", ctrl: true },
  "\x1b[1;5B": { name: "down", ctrl: true },
  "\x1b[1;5C": { name: "right", ctrl: true },
  "\x1b[1;5D": { name: "left", ctrl: true },
  "\x1b[1;2A": { name: "up", shift: true },
  "\x1b[1;2B": { name: "down", shift: true },
  "\x1b[1;2C": { name: "right", shift: true },
  "\x1b[1;2D": { name: "left", shift: true },
  "\x1b[1;3A": { name: "up", alt: true },
  "\x1b[1;3B": { name: "down", alt: true },
  "\x1b[1;3C": { name: "right", alt: true },
  "\x1b[1;3D": { name: "left", alt: true },
};

/**
 * Phân tích chuỗi raw byte từ terminal input thành đối tượng KeyEvent.
 */
export function parseKey(data: string): KeyEvent {
  // 1. Kiểm tra bracketed paste sequence: \x1b[200~ ... \x1b[201~
  if (data.startsWith("\x1b[200~") && data.endsWith("\x1b[201~")) {
    const content = data.slice(6, -6);
    return {
      name: content,
      ctrl: false,
      alt: false,
      shift: false,
      super: false,
      raw: data,
      isPaste: true,
    };
  }

  // 2. Tra cứu bảng escape sequences trực tiếp
  const directMatch = ESCAPE_SEQUENCES[data];
  if (directMatch) {
    return {
      name: directMatch.name,
      ctrl: directMatch.ctrl || false,
      alt: directMatch.alt || false,
      shift: directMatch.shift || false,
      super: false,
      raw: data,
    };
  }

  // 3. Phím đơn cơ bản
  if (data === "\r" || data === "\n") {
    return { name: "enter", ctrl: false, alt: false, shift: false, super: false, raw: data };
  }
  if (data === "\t") {
    return { name: "tab", ctrl: false, alt: false, shift: false, super: false, raw: data };
  }
  if (data === "\x1b") {
    return { name: "escape", ctrl: false, alt: false, shift: false, super: false, raw: data };
  }
  if (data === " ") {
    return { name: "space", ctrl: false, alt: false, shift: false, super: false, raw: data };
  }
  if (data === "\x7f" || data === "\x08") {
    return { name: "backspace", ctrl: false, alt: false, shift: false, super: false, raw: data };
  }

  // 4. Alt + phím (bắt đầu bằng \x1b)
  if (data.startsWith("\x1b") && data.length === 2) {
    const subChar = data[1].toLowerCase();
    return {
      name: subChar,
      ctrl: false,
      alt: true,
      shift: data[1] !== subChar,
      super: false,
      raw: data,
    };
  }

  // 5. Ctrl + phím ký tự (mã ASCII từ 1 đến 26 tương ứng a-z)
  if (data.length === 1) {
    const code = data.charCodeAt(0);
    if (code >= 1 && code <= 26) {
      const char = String.fromCharCode(96 + code); // 1 -> 'a'
      return {
        name: char,
        ctrl: true,
        alt: false,
        shift: false,
        super: false,
        raw: data,
      };
    }

    // Ký tự in thường hoặc hoa
    const isUpper = data >= "A" && data <= "Z";
    return {
      name: data.toLowerCase(),
      ctrl: false,
      alt: false,
      shift: isUpper,
      super: false,
      raw: data,
    };
  }

  // Mặc định trả về raw string
  return {
    name: data,
    ctrl: false,
    alt: false,
    shift: false,
    super: false,
    raw: data,
  };
}

/**
 * Kiểm tra xem phím vừa nhấn có khớp với pattern hay không (vd: "ctrl+c", "enter", "escape", "up")
 */
export function matchesKey(data: string, pattern: string): boolean {
  const event = parseKey(data);
  const parts = pattern.toLowerCase().split("+");
  const targetKey = parts[parts.length - 1];
  const reqCtrl = parts.includes("ctrl");
  const reqAlt = parts.includes("alt");
  const reqShift = parts.includes("shift");
  const reqSuper = parts.includes("super");

  return (
    event.name.toLowerCase() === targetKey &&
    event.ctrl === reqCtrl &&
    event.alt === reqAlt &&
    event.shift === reqShift &&
    event.super === reqSuper
  );
}
