import * as EventSourceModule from "eventsource";
const EventSource = (EventSourceModule as any).EventSource || (EventSourceModule as any).default || EventSourceModule;

export interface MetaInfo {
  protocolVersion: number;
  capabilities: string[];
  serverVersion: string;
  deviceProfile: string;
  sandbox: "none" | "restricted-token" | "full";
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

export interface ModelItem {
  id: string;
  name: string;
  provider: string;
  description?: string;
  contextWindow?: number;
}

export class AnankeClient {
  private baseUrl: string;
  private lastEventSeq = 0;
  private eventSource: EventSource | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private shouldReconnect = true;

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

  async listModels(): Promise<ModelItem[]> {
    const res = await fetch(`${this.baseUrl}/api/models`, {
      headers: this.headers(),
    });
    if (!res.ok) {
      throw new Error(`Failed to fetch models: ${res.status} ${res.statusText}`);
    }
    const data = await res.json();
    return (data.models || []) as ModelItem[];
  }

  async createSession(
    workspace: string = process.cwd(),
    model: string = "default"
  ): Promise<SessionResponse> {
    const res = await fetch(`${this.baseUrl}/api/session`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ workspace, model }),
    });
    if (!res.ok) {
      const errText = await res.text().catch(() => "");
      throw new Error(`Failed to create session: ${res.status} ${res.statusText} ${errText}`);
    }
    return (await res.json()) as SessionResponse;
  }

  async updateSession(
    sessionId: string,
    update: { permissionMode?: string; model?: string }
  ): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/session/${sessionId}`, {
      method: "PATCH",
      headers: this.headers(),
      body: JSON.stringify(update),
    });
    if (!res.ok && res.status !== 204) {
      throw new Error(`Failed to update session: ${res.status}`);
    }
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

  async resolvePermission(sessionId: string, permissionId: string, decision: "allow" | "deny" | "always"): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/session/${sessionId}/permission/${permissionId}`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({
        decision,
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
    onError: (err: any) => void
  ): () => void {
    this.shouldReconnect = true;

    const connect = () => {
      if (!this.shouldReconnect) return;
      const url = `${this.baseUrl}/api/session/${sessionId}/event?since=${this.lastEventSeq}`;
      // eventsource@2 tự dựng request bằng http/https và KHÔNG hỗ trợ option `fetch`.
      // Header phải truyền qua `headers`, nếu không server trả 426 protocol_mismatch.
      // `Last-Event-ID` được thư viện tự gắn từ options này vào request.
      const headers: Record<string, string> = { "X-Ananke-Protocol": "1" };
      if (this.lastEventSeq > 0) {
        headers["Last-Event-ID"] = String(this.lastEventSeq);
      }

      const es = new EventSource(url, { headers } as any);

      this.eventSource = es;

      es.onmessage = (msg: MessageEvent) => {
        try {
          const parsed = JSON.parse(msg.data) as EventItem;
          // Cập nhật seq nhưng không drop event broadcast không có seq hoặc seq lặp lại
          if (parsed.seq !== undefined && parsed.seq > this.lastEventSeq) {
            this.lastEventSeq = parsed.seq;
          }
          onEvent(parsed);
        } catch (e) {
          onError(e);
        }
      };

      es.onerror = (err: any) => {
        onError(err);
        es.close();
        this.eventSource = null;
        if (this.shouldReconnect && this.reconnectTimer === null) {
          this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = null;
            connect();
          }, 2000); // Tự động reconnect sau 2 giây với seq mới nhất
        }
      };
    };

    connect();

    return () => {
      this.shouldReconnect = false;
      if (this.reconnectTimer !== null) {
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = null;
      }
      if (this.eventSource) {
        this.eventSource.close();
        this.eventSource = null;
      }
    };
  }
}
