import * as readline from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";
import { AnankeClient, EventItem } from "./client.js";

// ANSI colors
const RESET = "\x1b[0m";
const BOLD = "\x1b[1m";
const DIM = "\x1b[2m";
const RED = "\x1b[31m";
const GREEN = "\x1b[32m";
const YELLOW = "\x1b[33m";
const BLUE = "\x1b[34m";
const CYAN = "\x1b[36m";

async function main() {
  const serverUrl = process.env.ANANKE_SERVER_URL || "http://127.0.0.1:9876";
  const client = new AnankeClient(serverUrl);

  console.log(`${BOLD}${CYAN}=== Ananke TUI Client (TypeScript / AD-12) ===${RESET}`);
  console.log(`${DIM}Connecting to ${serverUrl}...${RESET}`);

  let meta;
  try {
    meta = await client.getMeta();
    console.log(
      `${GREEN}✔ Connected!${RESET} Protocol: v${meta.protocolVersion} | Sandbox: ${
        meta.sandbox === "none" ? `${RED}${BOLD}none${RESET}` : `${GREEN}${meta.sandbox}${RESET}`
      } | DeviceProfile: ${YELLOW}${meta.deviceProfile}${RESET}`
    );
    if (meta.sandbox === "none") {
      console.log(
        `${YELLOW}⚠ Warning: Runtime sandbox is disabled or unsupported. Permission checks are active.${RESET}`
      );
    }
  } catch (err: any) {
    console.error(`${RED}✖ Failed to connect to Ananke server:${RESET}`, err.message);
    console.log(`${DIM}Hint: Run \`cargo run -p ananke-server\` in another terminal first.${RESET}`);
    process.exit(1);
  }

  const sessionId = await client.createSession(process.cwd(), "default");
  console.log(`${DIM}Session created: ${sessionId}${RESET}\n`);

  let currentTurnId: string | null = null;
  const rl = readline.createInterface({ input, output });

  // Subscribe SSE events
  client.subscribeEvents(
    sessionId,
    async (event: EventItem) => {
      const type = event.type;
      if (!type) return;

      if (type === "part.delta") {
        process.stdout.write(event.delta || "");
      } else if (type === "turn.started") {
        currentTurnId = event.turnID || null;
        console.log(`\n${CYAN}Agent thinking...${RESET}`);
      } else if (type === "turn.completed") {
        currentTurnId = null;
        console.log(`\n${GREEN}✔ Turn completed.${RESET}\n`);
      } else if (type === "turn.failed") {
        currentTurnId = null;
        console.log(`\n${RED}✖ Turn failed: ${event.reason || "unknown"}${RESET}\n`);
      } else if (type === "permission.requested") {
        const permId = event.permissionID;
        const toolName = event.tool || "unknown";
        console.log(
          `\n${YELLOW}${BOLD}⚠ Permission requested for tool: ${toolName}${RESET}`
        );
        console.log(`${DIM}Details: ${JSON.stringify(event.arguments || {})}${RESET}`);
        const answer = await rl.question(
          `${YELLOW}Allow this action? (y/N): ${RESET}`
        );
        const allow = answer.trim().toLowerCase() === "y";
        try {
          await client.resolvePermission(sessionId, permId, allow);
          console.log(
            allow
              ? `${GREEN}✔ Allowed tool execution.${RESET}`
              : `${RED}✖ Denied tool execution.${RESET}`
          );
        } catch (e: any) {
          console.error(`${RED}Error responding to permission:${RESET}`, e.message);
        }
      }
    },
    (err: any) => {
      // SSE connection errors / reconnecting
    }
  );

  // Command input loop
  while (true) {
    const prompt = await rl.question(`${BOLD}${BLUE}Ananke > ${RESET}`);
    const trimmed = prompt.trim();
    if (!trimmed) continue;
    if (trimmed === "/exit" || trimmed === "exit") {
      console.log("Goodbye!");
      process.exit(0);
    }
    if (trimmed === "/interrupt" && currentTurnId) {
      console.log(`${YELLOW}Interrupting turn...${RESET}`);
      await client.interruptTurn(sessionId, currentTurnId);
      continue;
    }

    try {
      await client.sendTurn(sessionId, trimmed);
    } catch (e: any) {
      console.error(`${RED}Error sending turn:${RESET}`, e.message);
    }
  }
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
