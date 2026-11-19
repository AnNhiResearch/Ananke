import EventSource from "eventsource";

export interface MetaInfo {
  protocolVersion: number;
  capabilities: string[];
  serverVersion: string;
  deviceProfile: string;
  sandbox: string;
}

export interface SessionResponse {
  id: string;
  model: string;
  workspace: string;
  worktree: string | null;
  permissionMode: string;
  createdAt: number;
}

export interface EventItem {
  seq: number;
  sessionID: string;
  at: number;
  turnID?: string;
  type?: string;
  [key: string]: any;
}

export class AnankeClient {
  private baseUrl: string;

  constructor(baseUrl: string = "http://127.0.0.1:9876") {
    this.baseUrl = baseUrl.replace(/\/$/, "");
  }

  private headers(extra: Record<string, string> = {}): Record<string, string> {
    return {
      "X-Ananke-Protocol": "1",
      "Content-Type": "application/json",
      ...extra,
    };
  }

  async getMeta(): Promise<MetaInfo> {
    const res = await fetch(`${this.baseUrl}/api/meta`, {
      headers: this.headers(),
    });
    if (!res.ok) {
      throw new Error(`Failed to fetch meta: ${res.status} ${res.statusText}`);
    }
    return (await res.json()) as MetaInfo;
  }

  async createSession(
    workspace: string = process.cwd(),
    model: string = "default"
  ): Promise<string> {
    const res = await fetch(`${this.baseUrl}/api/session`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ workspace, model }),
    });
    if (!res.ok) {
      const errText = await res.text().catch(() => "");
      throw new Error(`Failed to create session: ${res.status} ${res.statusText} ${errText}`);
    }
    const data = (await res.json()) as SessionResponse;
    return data.id;
  }

  async sendTurn(sessionId: string, text: string): Promise<string> {
    const res = await fetch(`${this.baseUrl}/api/session/${sessionId}/turn`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({
        input: [{ type: "text", text }],
      }),
    });
    if (!res.ok) {
      throw new Error(`Failed to create turn: ${res.status} ${res.statusText}`);
    }
    const data = (await res.json()) as { id: string };
    return data.id;
  }

  async resolvePermission(sessionId: string, permissionId: string, allow: boolean): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/session/${sessionId}/permission/${permissionId}`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({
        decision: allow ? "allow" : "deny",
      }),
    });
    if (!res.ok && res.status !== 204) {
      throw new Error(`Failed to resolve permission: ${res.status}`);
    }
  }

  async interruptTurn(sessionId: string, turnId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/session/${sessionId}/turn/${turnId}/interrupt`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok && res.status !== 204) {
      throw new Error(`Failed to interrupt turn: ${res.status}`);
    }
  }

  subscribeEvents(
    sessionId: string,
    onEvent: (event: EventItem) => void,
    onError: (err: any) => void,
    since: number = 0
  ): EventSource {
    const url = `${this.baseUrl}/api/session/${sessionId}/event?since=${since}`;
    const es = new EventSource(url, {
      headers: { "X-Ananke-Protocol": "1" },
    });

    es.onmessage = (msg: MessageEvent) => {
      try {
        const parsed = JSON.parse(msg.data) as EventItem;
        onEvent(parsed);
      } catch (e) {
        onError(e);
      }
    };

    es.onerror = (err: any) => {
      onError(err);
    };

    return es;
  }
}
