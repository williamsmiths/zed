# Proposal: Council (Multi‑Agent Shared Chat) + Work Items cho Zed

> **Status:** Draft / Proposal · **Ngày:** 2026‑06‑29 · **Phạm vi:** backend‑first, build thật trong cây nguồn Zed
> **Nguồn:** khảo sát code Zed (có `file:line`) + landscape đa‑agent ngoài (đánh dấu độ tin cậy ở §16).

Tài liệu này thiết kế một tính năng Zed cho phép **nhiều agent (và người) cùng tham gia một phòng thảo luận chung theo project** để phân tích vấn đề và **sinh ra một danh sách công việc (Work Items) hoàn chỉnh**, có thể theo dõi.

---

## 0. Tóm tắt & quyết định cốt lõi

1. **Không phục hồi ephemeral chat** — Zed đã chủ động gỡ. Mô hình "chat chung" = **blackboard có cấu trúc, append‑only**, mỗi phát biểu là một `council_entry` bất biến, sắp thứ tự bằng Lamport.
2. **Tri thức chung về codebase** = **adopt `codebase-memory` MCP** (knowledge graph), không tự build index — Zed hiện **không có** semantic index dày.
3. **Chống clobber** dựa trên hạ tầng có sẵn của Zed: CRDT ordering (`clock::Lamport`/`Global`) + `ActionLog` (đã theo dõi mọi sửa đổi của agent).
4. **Quản trị Hội đồng:** Super (người) tối thượng → 1 Supervisor (agent giỏi nhất) điều phối thay người → các Peer ngang quyền thảo luận.
5. **Điều phối đa‑agent là phần rủi ro nhất** (bằng chứng: Cognition, MAST, Anthropic). Triển khai theo **6 lát cắt tăng dần** (M1–M6), human‑in‑the‑loop, có kill‑switch.

---

## 1. Bối cảnh & phát biểu bài toán

Hiện mỗi phiên agent trong Zed (`Thread`, `crates/agent/src/thread.rs:1170`) là một ốc đảo: lịch sử riêng, context riêng, không thấy nhau. Khi mở nhiều agent / nhiều phiên trên cùng repo, thiếu **một nguồn sự thật chung** ("đang giải bài gì, đã quyết gì, biết gì về codebase") và thiếu **kênh để các agent trao đổi**.

Mục tiêu:
- **Shared chat (Council):** kênh nhiều agent + người cùng đọc/ghi, có vai trò & điều phối, hội tụ về một bản phân rã công việc.
- **Work Items:** materialize kết quả hội đồng thành danh sách task theo dõi được (todo / in‑progress / done).

---

## 2. Mô hình quản trị "Hội đồng"

| Vai | Là ai | Quyền | KHÔNG được |
|---|---|---|---|
| **Super** | Người dùng (bạn) | Phê duyệt cuối, sửa, phủ quyết, chốt; kill‑switch mọi lúc | — (tối thượng) |
| **Supervisor** | 1 agent giỏi nhất | Khung hoá, giao lượt, đặt câu hỏi, tổng hợp `task_draft`, tuyên bố hội tụ — *thay mặt Super* | Tự chốt, **trừ khi** Super bật autonomous (D1) |
| **Peer ×N** | Các agent còn lại, **ngang quyền** | Phân tích, phản biện lẫn nhau, đề xuất task | Điều phối, chốt |

**Luồng:** Super nêu vấn đề → Supervisor điều phối → Peers thảo luận/phản biện → Supervisor tổng hợp WBS → Super duyệt → chốt → materialize Work Items.

Mô hình = **AutoGen GroupChat** (Supervisor ≈ `GroupChatManager`) + **MetaGPT shared message pool** (blackboard) + **human‑in‑the‑loop gate** + nguyên tắc **"share full traces"** (Cognition). Đây là tổ hợp các mảnh đã chạy thật, ghép lên hạ tầng collab có sẵn của Zed.

---

## 3. Quyết định (D1–D4)

- **D1 — Quyền là công tắc do Super nắm.** Mặc định `human_final` (Super duyệt cuối). Khi Super bật `supervisor_autonomous`, Supervisor được **toàn quyền chốt**. Super luôn có **kill‑switch** kể cả ở chế độ autonomous.
- **D2 — Peers đa dạng & động.** Khác model/tool; join/leave giữa chừng.
- **D3 — Work Items tracker thật.** Lưu ý: crate `task` của Zed (`crates/task/src/task_template.rs:162`) là **runnable shell tasks**, KHÔNG phải tracker. Zed **không** có issue‑tracker in‑app. → tính năng mới, đặt tên `WorkItem` để tránh trùng.
- **D4 — Build trong cây nguồn Zed thật** (PR‑sized milestones M1–M6).

---

## 4. Hiện trạng kiến trúc Zed (tái dùng / thiếu)

### 4a. Nửa "chat / collaboration"
| Thành phần | Trạng thái | Vị trí |
|---|---|---|
| Collab server (WebSocket, sea‑orm + Postgres) | ✅ sống | `Server` `collab/src/rpc.rs:311`; upgrade `:1209` |
| Pub/sub: `ConnectionPool`, `subscribe_to_channel`, `broadcast` | ✅ sống | `collab/src/rpc/connection_pool.rs:11, :1097` |
| Channels: membership, role, visibility, hierarchy, presence | ✅ sống | `ChannelStore` `channel/src/channel_store.rs:36` |
| **Channel text‑chat (ephemeral)** | ❌ **đã gỡ** | `rpc.rs:3700/3753` trả lỗi "chat has been removed" |
| Collaborative buffer (CRDT) | ✅ sống | `buffer`/`buffer_operation`/`buffer_snapshot` tables |
| CRDT: `Lamport`, `Global`; **`ReplicaId::AGENT=2`** dành sẵn cho AI | ✅ sống | `clock/src/clock.rs:14, 63, 70` |

### 4b. Nửa "agent / memory"
| Thành phần | Trạng thái | Vị trí |
|---|---|---|
| Thread model + persistence (SQLite + zstd JSON) | ✅ | `agent/src/thread.rs:1170`, `agent/src/db.rs`; `~/.local/share/Zed/db/threads.db` |
| MCP client + registry | ✅ | `context_server/`; `ContextServerRegistry` `agent/src/tools/context_server_registry.rs:47`; config `settings.json → context_servers` |
| `AGENTS.md` (global + per‑project) — file context chung | ✅ | `load_worktree_rules_file()` `agent/src/agent.rs:1242` |
| `ActionLog` — theo dõi mọi file agent đọc/sửa | ✅ | `action_log/src/action_log.rs:52` |
| **Semantic / codebase index dày** | ❌ **không có** | chỉ BM25 `edit_prediction_context/src/bm25_context.rs:258`; `embeddings_dir()` chỉ là path sót `paths.rs:427` |

→ **Kết luận:** đường ống (server, pub/sub, CRDT, persistence, MCP, ActionLog, replica id cho agent) còn nguyên. Phải tự dựng: **lớp quản trị hội đồng** + **lớp work‑item**. Tri thức codebase chung → **adopt `codebase-memory` MCP**.

---

## 5. Kiến trúc tổng

```
        ┌─────────────────────────  ZED CLIENT  ─────────────────────────┐
        │  Agent Panel (Thread) ×N agent  +  Super (người)               │
        │     │ post/read                 │ approve / kill-switch        │
        │     ▼                           ▼                              │
        │  [L1] Shared Codebase-Memory    [L2] Council (blackboard)       │
        │   = codebase-memory MCP          = council_entry (append-only)  │
        │     (knowledge graph)              roles, phases, turn-taking   │
        │                                    │ materialize on approve     │
        │                                    ▼                            │
        │                                 [L3] Work Items (tracker, D3)   │
        └───────────────┬───────────────────────────┬────────────────────┘
                        │ MCP (stdio/JSON-RPC)       │ council.proto (RPC)
                        ▼                            ▼
                 codebase-memory MCP           collab server
                 (SQLite WAL,                  ConnectionPool.broadcast →
                  graph.db.zst team artifact)  council_entry / work_item (Postgres)
```

---

## 6. Mô hình dữ liệu (sea‑orm, Postgres collab)

```
council_session(id, project_id, supervisor_participant_id,
  phase ENUM(frame,diverge,converge,synthesize,gate,finalized),
  round INT, authority ENUM(human_final,supervisor_autonomous),     -- D1
  status ENUM(active,finalized,abandoned), created_at, updated_at)

council_participant(id, session_id, kind ENUM(super,supervisor,peer),  -- D2
  user_id NULL, agent_label, model, tool, replica_id,
  joined_at, left_at NULL)            -- left_at NULL = đang ở (join/leave động)

council_entry(id, session_id, author_participant_id,
  lamport_value INT, lamport_replica_id INT,
  kind ENUM(directive,analysis,critique,proposal,task_draft,approval),
  body TEXT, refs JSONB, created_at)  -- append-only, immutable

work_item(id, project_id, session_id, source_entry_id,            -- D3
  title, description, status ENUM(todo,in_progress,blocked,done,cancelled),
  assignee_participant_id NULL, parent_id NULL, sort_order, created_at, updated_at)
```

**Lý do `council_entry` là bảng có cấu trúc, append‑only (không CRDT text buffer):** entry bất biến sau khi đăng nên chỉ cần **Lamport ordering + server sequencing**, không cần merge CRDT — đơn giản hơn và truy vấn được để sinh `work_item`. (CRDT buffer vẫn để dành cho scratch‑doc tự do nếu cần sau.)

---

## 7. Giao thức `crates/proto/proto/council.proto`

```proto
JoinCouncil{project_id} -> CouncilState{session, participants[], entries[], work_items[]}
LeaveCouncil{session_id, participant_id}
PostCouncilEntry{session_id, kind, body, refs[]} -> EntryAck{id, lamport}
CouncilEntryPosted{entry}                          // broadcast
AdvanceCouncilPhase{session_id, phase}             // chỉ Supervisor
SetCouncilAuthority{session_id, authority}         // chỉ Super   (D1 toggle)
SubmitTaskDraft{session_id, items[]}               // Supervisor -> work_item pending
ApproveTaskDraft{session_id, draft_entry_id, decision}  // Super | Supervisor(autonomous)
UpsertWorkItem{item} / WorkItemUpdated{item}       // broadcast   (D3)
CouncilParticipantUpdated{participant}             // broadcast   (D2 join/leave)
```

---

## 8. Server handler + luật quyền (D1)

Đăng ký trong `Server::new()` (theo pattern channel handler hiện có, `collab/src/rpc.rs`). Lõi kiểm soát quyền cho hành vi chốt:

```rust
// khi nhận entry kind=Approval hoặc ApproveTaskDraft
let p = session.participant(author)?;
let allowed = p.kind == Super
   || (p.kind == Supervisor
       && session.authority == Authority::SupervisorAutonomous);   // D1
anyhow::ensure!(allowed, "only Super may finalize (or Supervisor if autonomous)");
```

Broadcast: `ConnectionPool::channel_connection_ids(session)` → `broadcast(ids, msg)` (tái dùng `connection_pool.rs:1097`).

---

## 9. Vòng đời phiên (state machine, Supervisor lái)

```
frame ──► diverge ──► converge ⇄ (critique rounds, round_cap)
                          │
                          ▼
                     synthesize ──► gate ──► finalized
                                      │  reject + feedback
                                      └──────► converge
```

- `AdvanceCouncilPhase` chỉ Supervisor; server validate chuyển pha hợp lệ.
- **Gate** phụ thuộc `authority`: `human_final` → chờ `Approval` của Super; `supervisor_autonomous` → Supervisor tự `Approval`.
- **Kill‑switch:** Super override mọi lúc, kể cả autonomous.

---

## 10. Định danh & replica cho agent (D2)

- Server cấp `replica_id` mỗi participant lúc join, lấy từ **dải collab ≥ `FIRST_COLLAB_ID(8)`** (`clock.rs:14`) để chạy **nhiều agent đồng thời** (`ReplicaId::AGENT=2` chỉ dành cho agent in‑process sửa buffer).
- `kind` + `model`/`tool` cho phép Peers **đa dạng** & **join/leave động** (set `left_at`).

---

## 11. Client & tham gia

- **Zed‑native:** crate `council` với `CouncilStore` (mirror `ChannelStore` `channel_store.rs:36`); một `Thread` tham gia → vòng lặp model đọc `entries` (qua `CouncilState`/`CouncilEntryPosted`) và ghi qua `PostCouncilEntry`.
- **CLI ngoài (D2 cross‑tool):** một **MCP "council" server** expose tool `council.join/read/post/whose_turn/propose_tasks` → dịch sang proto trên. Claude Code / Codex / Gemini CLI… join **cùng một phiên**, cùng đổ vào `council_entry`. Cũng là chỗ cắm `codebase-memory` MCP làm tri thức chung khi Peers phân tích.

---

## 12. Work Items (D3)

- Crate `work_item` (model + `WorkItemStore`) + `work_item_ui` (`WorkItemPanel`, mirror `project_panel`/`agent_panel`): danh sách nhóm theo `status`, sửa tay, gán `assignee` (người **hoặc** agent).
- **Liên kết council → task:** khi `ApproveTaskDraft` thành công, server **materialize** `work_item` rows từ `items` của draft → broadcast `WorkItemUpdated`. Super theo dõi/sửa trạng thái trong panel, đồng bộ realtime qua collab.
- Tên "Work Items" (không phải "Tasks") để tách bạch với runnable `task` crate.

---

## 13. Prompt khung

**Supervisor (system):** *"Bạn điều phối hội đồng thay mặt người dùng. Quyền hiện tại = `{authority}`; nếu `human_final`, bạn chỉ đề xuất, không tự chốt. Trách nhiệm: khung hoá vấn đề → giao lượt (post `directive` chỉ định 1 peer) → phát hiện hội tụ (≤ `round_cap`) → tổng hợp `task_draft` (WBS đầy đủ) → trình Super. Đọc `codebase-memory` MCP cho dữ kiện. Mọi entry là trace công khai."*

**Peer (system):** *"Bạn là nhà phân tích ngang quyền. Chỉ phát biểu khi được `directive` gọi, hoặc trong pha `diverge`. Post `analysis|critique|proposal`, dẫn `refs` tới entry khác, trích `codebase-memory` cho fact. Ngắn gọn, phản biện thẳng."*

---

## 14. Lát cắt triển khai (PR‑sized, vì D4 làm thật)

| # | Nội dung | Tiêu chí xong |
|---|---|---|
| **M1** | `council.proto` + migrations + handler session/participant/entry (no UI) | server build + test integration trong `collab` |
| **M2** | `CouncilStore` + 1 Zed agent join/post/nhận broadcast | 1 agent thảo luận được trên 1 phiên |
| **M3** | Authority toggle (D1) + state‑machine pha + Gate/Approval | luật quyền có test |
| **M4** | `work_item` + `WorkItemPanel` + materialize draft→task (D3) | theo dõi task end‑to‑end |
| **M5** | MCP council bridge (D2 cross‑CLI) | Claude Code join cùng phiên |
| **M6** | Profile Supervisor/Peer + đa‑peer + guardrail token/round | hội đồng nhiều agent chạy thật |

Mỗi M là 1 PR theo chuẩn repo: title mệnh lệnh, prefix crate, có mục `Release Notes:`.

---

## 15. Rủi ro & guardrail

- **Token (~15× — Anthropic):** chặn bằng `round_cap` + Supervisor giao‑lượt (không để mọi agent nói cùng lúc).
- **Inter‑agent misalignment (MAST):** trace công khai + entry có cấu trúc (`kind`) thay vì free‑form (tránh "telephone game" — MetaGPT).
- **"Giả định mâu thuẫn" (Cognition):** mọi participant đọc full blackboard.
- **Autonomous mode (D1):** luôn có kill‑switch của Super.

---

## 16. Nguồn tham chiếu

**Code Zed (verified, `file:line` trong tài liệu):** `collab/src/rpc.rs`, `…/rpc/connection_pool.rs`, `…/db/{tables,queries}`, `crates/proto/proto/{channel,buffer}.proto`, `crates/clock/src/clock.rs`, `crates/text/src/text.rs`, `crates/channel/src/channel_store.rs`, `crates/agent/src/{thread.rs,db.rs,agent.rs,templates.rs}`, `…/tools/context_server_registry.rs`, `crates/context_server/`, `crates/action_log/src/action_log.rs`, `crates/prompt_store/src/prompt_store.rs`, `crates/task/src/task_template.rs`, `crates/paths/src/paths.rs`.

**Landscape ngoài:**
- ✅ verified — `codebase-memory` MCP ([DeusData](https://github.com/DeusData/codebase-memory-mcp)); agentmemory ([rohitg00](https://github.com/rohitg00/agentmemory)).
- ◽ gathered (auto‑verify bị rate‑limit) — [AGENTS.md](https://agents.md/), [MetaGPT (arXiv 2308.00352)](https://arxiv.org/abs/2308.00352), [Anthropic multi‑agent](https://www.anthropic.com/engineering/multi-agent-research-system), [Cognition — Don't Build Multi‑Agents](https://cognition.com/blog/dont-build-multi-agents), [MAST (arXiv 2503.13657)](https://arxiv.org/html/2503.13657v1).
