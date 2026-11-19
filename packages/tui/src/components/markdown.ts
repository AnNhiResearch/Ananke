import { Component } from "../tui-core/differential.js";
import { colors, visualWidth } from "../tui-core/colors.js";

/**
 * StreamingMarkdown Component:
 * - Tiếp nhận token/delta streaming mượt mà từ SSE response (`part.delta` / `text`).
 * - Hỗ trợ highlight Markdown cơ bản:
 *   + Tiêu đề #, ##, ### (Bold + Cyan)
 *   + Code blocks ``` ... ``` (Gray border + Yellow code)
 *   + Inline code `code` (Yellow)
 *   + List item bullets -, *, 1. (Green bullet)
 *   + Bold **text** / Italic *text*
 * - Tự động word wrap theo chiều rộng khả dụng của terminal.
 */
export class StreamingMarkdown implements Component {
  private rawContent = "";

  constructor(initialContent = "") {
    this.rawContent = initialContent;
  }

  append(delta: string): void {
    this.rawContent += delta;
  }

  setContent(content: string): void {
    this.rawContent = content;
  }

  clear(): void {
    this.rawContent = "";
  }

  getText(): string {
    return this.rawContent;
  }

  render(width: number): string[] {
    const safeWidth = Math.max(20, width);
    const rawLines = this.rawContent.split("\n");
    const rendered: string[] = [];
    let inCodeBlock = false;
    let codeLanguage = "";

    for (const line of rawLines) {
      // 1. Code block delimiter ```
      if (line.trim().startsWith("```")) {
        if (!inCodeBlock) {
          inCodeBlock = true;
          codeLanguage = line.trim().slice(3).trim();
          const tag = codeLanguage ? `[${codeLanguage}]` : "";
          rendered.push(colors.gray(`┌── ${tag}${"─".repeat(Math.max(0, safeWidth - visualWidth(tag) - 6))}┐`));
        } else {
          inCodeBlock = false;
          rendered.push(colors.gray(`└───${"─".repeat(Math.max(0, safeWidth - 6))}┘`));
        }
        continue;
      }

      // 2. Nội dung trong code block
      if (inCodeBlock) {
        rendered.push(colors.gray("│ ") + colors.yellow(line));
        continue;
      }

      // 3. Tiêu đề Markdown
      // Mỗi phần tử trả về phải là ĐÚNG một dòng màn hình (không chứa "\n"),
      // nếu không DifferentialRenderer sẽ tính sai số dòng và in lặp.
      if (line.startsWith("# ")) {
        rendered.push("", colors.bold(colors.cyan(`● ${line.slice(2).trim()}`)));
        continue;
      }
      if (line.startsWith("## ")) {
        rendered.push("", colors.bold(colors.blue(`  ◆ ${line.slice(3).trim()}`)));
        continue;
      }
      if (line.startsWith("### ")) {
        rendered.push("", colors.bold(colors.magenta(`    ▲ ${line.slice(4).trim()}`)));
        continue;
      }

      // 4. Bullet lists
      if (/^\s*[-*]\s+/.test(line)) {
        const bulletLine = line.replace(/^\s*[-*]\s+/, "  " + colors.green("• ") );
        rendered.push(this.formatInline(bulletLine));
        continue;
      }

      // 5. Numbered lists
      if (/^\s*\d+\.\s+/.test(line)) {
        const numLine = line.replace(/^(\s*)(\d+\.)\s+/, (_m, s, num) => `${s}${colors.green(num)} `);
        rendered.push(this.formatInline(numLine));
        continue;
      }

      // 6. Blockquote
      if (line.startsWith("> ")) {
        rendered.push(colors.gray("┃ ") + colors.dim(this.formatInline(line.slice(2))));
        continue;
      }

      // 7. Đoạn văn bản thông thường
      if (line.trim().length === 0) {
        rendered.push("");
      } else {
        rendered.push(this.formatInline(line));
      }
    }

    return rendered;
  }

  private formatInline(text: string): string {
    // Inline code `...`
    let formatted = text.replace(/`([^`]+)`/g, (_match, code) => colors.yellow(` ${code} `));
    // Bold **...**
    formatted = formatted.replace(/\*\*([^*]+)\*\*/g, (_match, bold) => colors.bold(bold));
    // Italic *...*
    formatted = formatted.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, (_match, italic) => colors.italic(italic));
    return formatted;
  }
}
