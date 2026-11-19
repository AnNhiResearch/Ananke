/**
 * Bộ công cụ xử lý màu sắc và kiểu chữ ANSI cho Terminal
 */

export const ANSI = {
  reset: "\x1b[0m",
  bold: "\x1b[1m",
  dim: "\x1b[2m",
  italic: "\x1b[3m",
  underline: "\x1b[4m",
  inverse: "\x1b[7m",
  strikethrough: "\x1b[9m",

  // Foreground
  black: "\x1b[30m",
  red: "\x1b[31m",
  green: "\x1b[32m",
  yellow: "\x1b[33m",
  blue: "\x1b[34m",
  magenta: "\x1b[35m",
  cyan: "\x1b[36m",
  white: "\x1b[37m",
  gray: "\x1b[90m",

  // Bright Foreground
  brightRed: "\x1b[91m",
  brightGreen: "\x1b[92m",
  brightYellow: "\x1b[93m",
  brightBlue: "\x1b[94m",
  brightMagenta: "\x1b[95m",
  brightCyan: "\x1b[96m",
  brightWhite: "\x1b[97m",

  // Background
  bgBlack: "\x1b[40m",
  bgRed: "\x1b[41m",
  bgGreen: "\x1b[42m",
  bgYellow: "\x1b[43m",
  bgBlue: "\x1b[44m",
  bgMagenta: "\x1b[45m",
  bgCyan: "\x1b[46m",
  bgWhite: "\x1b[47m",
};

/**
 * Tạo màu RGB truecolor (24-bit) cho chữ
 */
export function fgRgb(r: number, g: number, b: number): string {
  return `\x1b[38;2;${r};${g};${b}m`;
}

/**
 * Tạo màu RGB truecolor (24-bit) cho nền
 */
export function bgRgb(r: number, g: number, b: number): string {
  return `\x1b[48;2;${r};${g};${b}m`;
}

/**
 * Helper bọc text với màu sắc tiện lợi
 */
export const style = {
  bold: (s: string) => `${ANSI.bold}${s}${ANSI.reset}`,
  dim: (s: string) => `${ANSI.dim}${s}${ANSI.reset}`,
  italic: (s: string) => `${ANSI.italic}${s}${ANSI.reset}`,
  underline: (s: string) => `${ANSI.underline}${s}${ANSI.reset}`,
  inverse: (s: string) => `${ANSI.inverse}${s}${ANSI.reset}`,
  red: (s: string) => `${ANSI.red}${s}${ANSI.reset}`,
  green: (s: string) => `${ANSI.green}${s}${ANSI.reset}`,
  yellow: (s: string) => `${ANSI.yellow}${s}${ANSI.reset}`,
  blue: (s: string) => `${ANSI.blue}${s}${ANSI.reset}`,
  magenta: (s: string) => `${ANSI.magenta}${s}${ANSI.reset}`,
  cyan: (s: string) => `${ANSI.cyan}${s}${ANSI.reset}`,
  white: (s: string) => `${ANSI.white}${s}${ANSI.reset}`,
  gray: (s: string) => `${ANSI.gray}${s}${ANSI.reset}`,
  bgRed: (s: string) => `${ANSI.bgRed}${s}${ANSI.reset}`,
  bgBlue: (s: string) => `${ANSI.bgBlue}${s}${ANSI.reset}`,
  brightGreen: (s: string) => `${ANSI.brightGreen}${s}${ANSI.reset}`,
  brightYellow: (s: string) => `${ANSI.brightYellow}${s}${ANSI.reset}`,
  brightCyan: (s: string) => `${ANSI.brightCyan}${s}${ANSI.reset}`,
};

export const colors = style;

// Regex loại bỏ escape sequence ANSI
const ANSI_REGEX =
  /[\u001b\u009b][[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]/g;

/**
 * Xóa toàn bộ chuỗi escape code ANSI khỏi text
 */
export function stripAnsi(text: string): string {
  return text.replace(ANSI_REGEX, "");
}

// Segmenter chia nhỏ grapheme cluster để đo chiều rộng chính xác
const graphemeSegmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });

/**
 * Đo chiều rộng hiển thị trực quan của 1 ký tự / cụm ký tự trên terminal.
 * CJK, full-width và Emoji chiếm 2 cột; ký tự ASCII/latinh chiếm 1 cột; ký tự điều khiển/zero-width chiếm 0 cột.
 */
export function charWidth(char: string): number {
  if (char.length === 0) return 0;
  const cp = char.codePointAt(0);
  if (cp === undefined) return 0;

  // Zero-width & control codes
  if (cp < 32 || (cp >= 0x7f && cp < 0xa0)) return 0;
  if (cp === 0x200b || cp === 0xfeff) return 0; // zero-width space, BOM

  // Fullwidth / CJK ranges
  if (
    (cp >= 0x1100 && cp <= 0x115f) || // Hangul Jamo
    (cp >= 0x2e80 && cp <= 0xa4cf && cp !== 0x303f) || // CJK Radicals, Kangxi, Hiragana, Katakana, Bopomofo, CJK Unified
    (cp >= 0xac00 && cp <= 0xd7a3) || // Hangul Syllables
    (cp >= 0xf900 && cp <= 0xfaff) || // CJK Compatibility Ideographs
    (cp >= 0xfe10 && cp <= 0xfe19) || // Vertical forms
    (cp >= 0xfe30 && cp <= 0xfe6f) || // CJK Compatibility Forms
    (cp >= 0xff00 && cp <= 0xff60) || // Fullwidth Forms
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f000 && cp <= 0x1fbff) || // Emoji & Pictographs
    (cp >= 0x2600 && cp <= 0x27bf) // Misc symbols & Dingbats
  ) {
    return 2;
  }

  return 1;
}

/**
 * Đo chiều rộng trực quan của toàn bộ chuỗi (sau khi bỏ ANSI)
 */
export function visualWidth(text: string): number {
  const clean = stripAnsi(text);
  let width = 0;
  for (const { segment } of graphemeSegmenter.segment(clean)) {
    width += charWidth(segment);
  }
  return width;
}

/**
 * Cắt chuỗi theo visual width tối đa, giữ lại hoặc không giữ ANSI
 */
export function truncateToVisualWidth(text: string, maxWidth: number): string {
  if (maxWidth <= 0) return "";
  let currentWidth = 0;
  let result = "";

  for (const { segment } of graphemeSegmenter.segment(text)) {
    const w = visualWidth(segment);
    if (currentWidth + w > maxWidth) break;
    result += segment;
    currentWidth += w;
  }
  return result;
}

/**
 * Đệm thêm khoảng trắng để đạt visual width mong muốn
 */
export function padEndVisual(text: string, targetWidth: number): string {
  const current = visualWidth(text);
  if (current >= targetWidth) return text;
  return text + " ".repeat(targetWidth - current);
}

interface AnsiPiece {
  value: string;
  width: number;
  ansi: boolean;
}

const ANSI_TEST_REGEX = new RegExp(`^(?:${ANSI_REGEX.source})$`);

/**
 * Tách chuỗi thành các "mảnh": escape sequence ANSI (giữ nguyên, width 0)
 * và grapheme cluster (kèm visual width).
 */
function tokenizeAnsi(text: string): AnsiPiece[] {
  const pieces: AnsiPiece[] = [];
  const splitter = new RegExp(`(${ANSI_REGEX.source})`, "g");

  for (const part of text.split(splitter)) {
    if (!part) continue;
    const first = part.charCodeAt(0);
    if ((first === 0x1b || first === 0x9b) && ANSI_TEST_REGEX.test(part)) {
      pieces.push({ value: part, width: 0, ansi: true });
      continue;
    }
    for (const { segment } of graphemeSegmenter.segment(part)) {
      pieces.push({ value: segment, width: charWidth(segment), ansi: false });
    }
  }

  return pieces;
}

/**
 * Ngắt một dòng thành nhiều dòng vật lý không vượt quá `maxWidth` cột,
 * giữ nguyên mã ANSI (tự áp lại style đang mở cho phần tiếp theo).
 * Ưu tiên ngắt ở khoảng trắng; từ quá dài thì ngắt cứng.
 */
export function wrapAnsi(text: string, maxWidth: number): string[] {
  const width = Math.max(1, Math.floor(maxWidth));
  const pieces = tokenizeAnsi(text);
  const out: string[] = [];

  let line: AnsiPiece[] = [];
  let lineWidth = 0;
  let activeCodes: string[] = [];

  const emit = (slice: AnsiPiece[]): void => {
    let rendered = slice.map((p) => p.value).join("");
    if (activeCodes.length > 0) rendered += ANSI.reset;
    out.push(rendered);
  };

  let i = 0;
  while (i < pieces.length) {
    const piece = pieces[i];

    if (piece.ansi) {
      if (piece.value === ANSI.reset || piece.value === "\x1b[m") {
        activeCodes = [];
      } else {
        activeCodes.push(piece.value);
      }
      line.push(piece);
      i++;
      continue;
    }

    if (lineWidth + piece.width > width && lineWidth > 0) {
      // Tìm khoảng trắng gần nhất để ngắt cho đẹp
      let breakIdx = line.length;
      for (let k = line.length - 1; k > 0; k--) {
        if (!line[k].ansi && line[k].value === " ") {
          breakIdx = k;
          break;
        }
      }

      const head = breakIdx < line.length ? line.slice(0, breakIdx) : line;
      const tail = breakIdx < line.length ? line.slice(breakIdx + 1) : [];
      // Dòng toàn khoảng trắng thì ngắt cứng, tránh sinh dòng rỗng
      emit(head.length > 0 ? head : line);

      // Dòng tiếp theo phải mở lại các style đang active
      line = activeCodes
        .map((code) => ({ value: code, width: 0, ansi: true }))
        .concat(tail);
      lineWidth = tail.reduce((sum, p) => sum + p.width, 0);
      continue; // xử lý lại piece hiện tại trên dòng mới
    }

    line.push(piece);
    lineWidth += piece.width;
    i++;
  }

  if (line.length > 0) emit(line);
  if (out.length === 0) out.push("");
  return out;
}
