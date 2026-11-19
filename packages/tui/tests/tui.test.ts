import { describe, it, expect } from "bun:test";
import { stripAnsi, visualWidth, colors } from "../src/tui-core/colors.js";
import { parseKey, matchesKey } from "../src/tui-core/keys.js";
import { Editor } from "../src/components/editor.js";
import { StreamingMarkdown } from "../src/components/markdown.js";
import { ApprovalDialog } from "../src/components/approval.js";
import { ModelSelector } from "../src/components/model-selector.js";
import { SlashMenu } from "../src/components/slash-menu.js";
import { ThinkBlock } from "../src/components/think-block.js";

describe("Colors & Visual Width", () => {
  it("strips ANSI escape codes cleanly", () => {
    const raw = colors.red("hello") + " " + colors.bold(colors.blue("world"));
    expect(stripAnsi(raw)).toBe("hello world");
  });

  it("calculates visual width correctly for ASCII and CJK/Emoji", () => {
    expect(visualWidth("abc")).toBe(3);
    expect(visualWidth("Xin chào")).toBe(8);
    // CJK character visual width = 2
    expect(visualWidth("你好")).toBe(4);
    // Emoji visual width = 2
    expect(visualWidth("🚀")).toBe(2);
  });
});

describe("Key Parsing", () => {
  it("parses basic arrow keys", () => {
    expect(parseKey("\x1b[A").name).toBe("up");
    expect(parseKey("\x1b[B").name).toBe("down");
    expect(parseKey("\x1b[C").name).toBe("right");
    expect(parseKey("\x1b[D").name).toBe("left");
  });

  it("parses enter, backspace, and escape", () => {
    expect(parseKey("\r").name).toBe("enter");
    expect(parseKey("\x7f").name).toBe("backspace");
    expect(parseKey("\x1b").name).toBe("escape");
  });

  it("detects key matches with modifiers", () => {
    expect(matchesKey("\x03", "ctrl+c")).toBe(true);
    expect(matchesKey("\r", "enter")).toBe(true);
  });

  it("parses bracketed paste block", () => {
    const pasted = parseKey("\x1b[200~Hello Ananke\x1b[201~");
    expect(pasted.isPaste).toBe(true);
    expect(pasted.name).toBe("Hello Ananke");
  });
});

describe("Editor Component", () => {
  it("handles text input, newlines, and clearing", () => {
    const editor = new Editor();
    editor.handleInput("h");
    editor.handleInput("i");
    expect(editor.getText()).toBe("hi");

    // Backspace
    editor.handleInput("\x7f");
    expect(editor.getText()).toBe("h");

    // Enter submits and clears
    let submitted = "";
    const submitEditor = new Editor({ onSubmit: (t) => (submitted = t) });
    submitEditor.handleInput("a");
    submitEditor.handleInput("\r");
    expect(submitted).toBe("a");
    expect(submitEditor.getText()).toBe("");

    editor.handleInput(" ");
    editor.handleInput("e");
    editor.handleInput("y");
    expect(editor.getText()).toBe("h ey");
  });
});

describe("StreamingMarkdown Component", () => {
  it("renders headers and code block highlights", () => {
    const md = new StreamingMarkdown();
    md.append("# Header 1\n");
    md.append("```rust\nlet x = 10;\n```\n");
    const lines = md.render(80);
    expect(lines.length).toBeGreaterThan(0);
    expect(stripAnsi(lines.join("\n"))).toContain("Header 1");
    expect(stripAnsi(lines.join("\n"))).toContain("let x = 10;");
  });
});

describe("ApprovalDialog Component", () => {
  it("triggers correct decision callback on key inputs", () => {
    let chosen = "";
    const dialog = new ApprovalDialog({
      request: {
        toolCallId: "perm-1",
        toolName: "bash",
        arguments: { command: "ls" },
      },
      onDecision: (d) => (chosen = d),
    });

    // Press 'y' -> allow
    dialog.handleInput("y");
    expect(chosen).toBe("allow");

    // Press 'n' -> deny
    dialog.handleInput("n");
    expect(chosen).toBe("deny");

    // Press 'w' -> always
    dialog.handleInput("w");
    expect(chosen).toBe("always");
  });
});

describe("ModelSelector Component", () => {
  it("navigates and selects model correctly", () => {
    let selected = "";
    const selector = new ModelSelector({
      models: [
        { id: "gpt-4o", name: "GPT-4o", provider: "openai" },
        { id: "claude-3-5-sonnet", name: "Claude 3.5 Sonnet", provider: "anthropic" },
      ],
      currentModel: "gpt-4o",
      onSelect: (id) => (selected = id),
      onCancel: () => {},
    });

    // Arrow down -> selects second
    selector.handleInput("\x1b[B");
    selector.handleInput("\r");
    expect(selected).toBe("claude-3-5-sonnet");
  });
});

describe("SlashMenu Component", () => {
  it("filters and autocompletes slash commands", () => {
    let chosen = "";
    const menu = new SlashMenu({
      commands: [
        { name: "help", description: "Show help" },
        { name: "model", description: "Select model" },
        { name: "clear", description: "Clear screen" },
      ],
      filter: "/m",
      onSelect: (cmd) => (chosen = cmd),
      onClose: () => {},
    });

    const lines = menu.render(80);
    expect(lines.length).toBeGreaterThan(0);

    menu.handleInput("\r");
    expect(chosen).toBe("/model");
  });
});

describe("ThinkBlock Component", () => {
  it("renders collapsed by default and toggles on demand", () => {
    const block = new ThinkBlock({ content: "Step 1: thinking\nStep 2: verify" });
    const collapsed = block.render(80);
    expect(collapsed[0]).toContain("Thinking...");

    block.toggle();
    const expanded = block.render(80);
    expect(expanded.join("\n")).toContain("Step 1: thinking");
  });
});
