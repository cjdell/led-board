import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js";
import {
  type Animation,
  animationKind,
  animationSummary,
  boardStatus,
  downloadText,
  formatDuration,
  GlobalDeviceApi,
  guardAsync,
  humanise,
  livePlaylist,
  normalisePlaylist,
  pickTextFile,
  type PlaylistEntry,
  type PlaylistItem,
  toast,
} from "@lib";
import { Body, Footer, Panel, PanelHeader, Spacer } from "../components/Panel.tsx";
import { Button } from "../components/Button.tsx";
import { Dialog } from "../components/Dialog.tsx";
import { LedMatrix } from "../components/LedMatrix.tsx";
import { AnimParamsEditor } from "../components/AnimParamsEditor.tsx";

/// Mobile tab state (module-level so it survives route changes).
const [view, setView] = createSignal<"editor" | "library" | "saved">("editor");

export function PlaylistsRoute() {
  const api = GlobalDeviceApi;

  // ── draft / board sync ───────────────────────────────────────────────────
  // The board's live playlist seeds the draft; while the draft is untouched
  // it keeps tracking the board (another tab hitting Play updates this one).
  // Once edited, the draft holds its ground until Play/Save/Discard.

  const [draft, setDraft] = createSignal<PlaylistItem[] | null>(null);
  const [baseline, setBaseline] = createSignal("");

  const live = createMemo(() => livePlaylist());

  createEffect(() => {
    const pl = live();

    if (!pl) return;

    const json = JSON.stringify(pl.playlist);

    if (json === baseline()) return;

    const d = draft();

    // Untouched draft (or none yet) tracks the board; an edited one does not.
    if (d === null || JSON.stringify(d) === baseline()) {
      setDraft(pl.playlist);
      setBaseline(json);
    }
  });

  const dirty = createMemo(() => draft() !== null && JSON.stringify(draft()) !== baseline());

  const totalDuration = createMemo(() => (draft() ?? []).reduce((sum, item) => sum + item.duration_ms, 0));

  const mutate = (fn: (items: PlaylistItem[]) => PlaylistItem[]) => {
    setDraft((items) => (items ? fn([...items]) : items));
  };

  // ── actions ──────────────────────────────────────────────────────────────

  const sendPlaylist = guardAsync(async (save: boolean) => {
    const items = draft();

    if (!items) return;

    await api.send({ Playlist: { playlist: items, save } });
    setBaseline(JSON.stringify(items));
    toast(save ? "Playlist saved to the board ✓" : "Playlist is playing ✓", "ok");
  });

  const resetToDefault = guardAsync(async () => {
    await api.send("Reset");
    toast("Festival default playlist restored", "ok");
  });

  const discard = () => {
    const pl = live();

    if (pl) {
      setDraft(pl.playlist);
      setBaseline(JSON.stringify(pl.playlist));
      toast("Reverted to what the board is playing");
    }
  };

  const exportDraft = () => {
    if (!draft()) return;

    downloadText("playlist.json", JSON.stringify(draft(), null, 2));
  };

  const importFile = guardAsync(async () => {
    const text = await pickTextFile();
    const items = normalisePlaylist(JSON.parse(text));

    if (items.length === 0) throw new Error("That file has no playlist items");

    setDraft(items);
    toast(`Loaded ${items.length} item(s) from file — press Play to try it`, "ok");
  });

  // ── save-as dialog ───────────────────────────────────────────────────────

  const [saveOpen, setSaveOpen] = createSignal(false);
  const [saveName, setSaveName] = createSignal("");

  const [library, { refetch: refetchLibrary }] = createResource(() => api.listPlaylists());

  const doSaveAs = guardAsync(async () => {
    const name = saveName().trim();

    if (!name) throw new Error("Give the playlist a name");
    if (!draft()) return;

    await api.putPlaylist(name, draft()!);
    refetchLibrary();
    toast(`Saved "${name}" to the board's library ✓`, "ok");
    setSaveName("");
  });

  const loadLibraryItem = guardAsync(async (entry: PlaylistEntry) => {
    const items = await api.fetchPlaylist(entry.name);

    setDraft(items);
    await api.send({ Playlist: { playlist: items, save: false } });
    setBaseline(JSON.stringify(items));
    toast(`"${entry.name}" is playing — Save to make it the board default`, "ok");
  });

  const setLibraryDefault = guardAsync(async (entry: PlaylistEntry) => {
    const items = await api.fetchPlaylist(entry.name);

    setDraft(items);
    await api.send({ Playlist: { playlist: items, save: true } });
    setBaseline(JSON.stringify(items));
    toast(`"${entry.name}" is now the board default ✓`, "ok");
  });

  const [confirmDelete, setConfirmDelete] = createSignal<string | null>(null);

  const deleteLibraryItem = guardAsync(async (name: string) => {
    await api.deletePlaylist(name);
    refetchLibrary();
    setConfirmDelete(null);
    toast(`Deleted "${name}"`, "ok");
  });

  // ── editor list ──────────────────────────────────────────────────────────

  const [dragIndex, setDragIndex] = createSignal<number | null>(null);
  const [dropIndex, setDropIndex] = createSignal<number | null>(null);

  const moveTo = (from: number, to: number) => {
    mutate((items) => {
      if (from < 0 || from >= items.length) return items;

      const [item] = items.splice(from, 1);

      items.splice(Math.max(0, Math.min(to, items.length)), 0, item);

      return items;
    });
  };

  const patch = (i: number, next: Partial<PlaylistItem>) => {
    mutate((items) => {
      items[i] = { ...items[i], ...next };

      return items;
    });
  };

  const liveIndex = createMemo(() => (dirty() ? -1 : (live()?.index ?? -1)));

  return (
    <div class="page">
      <div class="btn-row subtabs">
        <TabButton active={view() === "editor"} click={() => setView("editor")}>
          Editor
        </TabButton>
        <TabButton active={view() === "library"} click={() => setView("library")}>
          Animations
        </TabButton>
        <TabButton active={view() === "saved"} click={() => setView("saved")}>
          Saved
        </TabButton>
      </div>

      {/* Editor + right column share one grid on wide screens. */}
      <div class="split split--uneven">
        <div class={view() === "editor" ? "" : "hide-narrow"}>
          <Panel accent={dirty() ? "amber" : undefined}>
            <PanelHeader>
              Playlist
              <Spacer />
              <span class="chip">{draft()?.length ?? 0} items</span>
              <span class="chip">{formatDuration(totalDuration())}</span>
              <Show when={dirty()}>
                <span class="chip chip--amber">unsaved edits</span>
              </Show>
              <Show when={live()?.source}>
                {(src) => <span class="chip">{src() === "Saved" ? "on flash" : src().toLowerCase()}</span>}
              </Show>
            </PanelHeader>

            <Body>
              <Show when={draft()?.length} fallback={<div class="empty">Nothing queued — add animations from the library →</div>}>
                <div class="list pl-editor">
                  <For each={draft() ?? []}>
                    {(item, i) => (
                      <div
                        class={`pl-row ${liveIndex() === i() ? "pl-row--active" : ""} ${dragIndex() === i() ? "pl-row--dragging" : ""} ${
                          dropIndex() === i() ? "pl-row--dropon" : ""
                        }`}
                        onDragOver={(e) => {
                          e.preventDefault();
                          setDropIndex(i());
                        }}
                        onDragLeave={() => setDropIndex(null)}
                        onDrop={(e) => {
                          e.preventDefault();

                          if (dragIndex() !== null) moveTo(dragIndex()!, i());

                          setDragIndex(null);
                          setDropIndex(null);
                        }}
                      >
                        <div class="pl-row__left">
                          <span
                            class="pl-row__grip"
                            draggable
                            onDragStart={() => setDragIndex(i())}
                            onDragEnd={() => {
                              setDragIndex(null);
                              setDropIndex(null);
                            }}
                            title="Drag to reorder"
                          >
                            ⠿
                          </span>
                          <span class="row-index">{i() + 1}</span>
                        </div>

                        <div class="pl-row__body">
                          <div class="pl-row__titlerow">
                            <input
                              class="input-inline pl-row__label"
                              placeholder={`${humanise(animationKind(item.animation))}${
                                animationSummary(item.animation) ? ` — ${animationSummary(item.animation)}` : ""
                              }`}
                              value={item.label ?? ""}
                              onInput={(e) => patch(i(), { label: e.currentTarget.value || undefined })}
                            />
                            <Show when={liveIndex() === i()}>
                              <span class="chip chip--amber">playing</span>
                            </Show>
                          </div>
                          <div class="pl-row__subrow">
                            <span class="pl-row__kind truncate">{humanise(animationKind(item.animation))}</span>
                            <span class="hstack" style={{ gap: "3px", "flex-shrink": "0" }}>
                              <input
                                class="input-inline input--number pl-row__duration"
                                type="number"
                                min="1"
                                step="1"
                                value={Math.round(item.duration_ms / 1000)}
                                onInput={(e) => patch(i(), { duration_ms: Math.max(1, Number(e.currentTarget.value)) * 1000 })}
                              />
                              <span class="pl-row__dur-unit">s</span>
                            </span>
                          </div>
                        </div>

                        <div class="pl-row__actions">
                          <Button
                            variant="ghost"
                            size="icon"
                            title="Play this now"
                            click={() => api.send({ JumpTo: i() }).catch(() => toast("Board offline", "error"))}
                          >
                            ▶
                          </Button>
                          <Button variant="ghost" size="icon" title="Move up" click={() => moveTo(i(), i() - 1)}>
                            ↑
                          </Button>
                          <Button variant="ghost" size="icon" title="Move down" click={() => moveTo(i(), i() + 1)}>
                            ↓
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            title="Duplicate"
                            click={() => mutate((items) => (items.splice(i() + 1, 0, { ...item }), items))}
                          >
                            ⧉
                          </Button>
                          <Button
                            variant="danger"
                            size="icon"
                            title="Remove"
                            click={() => mutate((items) => items.filter((_, idx) => idx !== i()))}
                          >
                            ✕
                          </Button>
                        </div>
                      </div>
                    )}
                  </For>
                </div>
              </Show>
            </Body>

            <Footer>
              <Show when={dirty()}>
                <Button variant="ghost" click={discard}>
                  Revert to board
                </Button>
              </Show>
              <Button variant="ghost" click={importFile}>
                Import…
              </Button>
              <Button variant="ghost" click={exportDraft}>
                Export
              </Button>
              <Button variant="danger" click={resetToDefault}>
                Restore default
              </Button>
              <Button variant="cyan" click={() => sendPlaylist(false)}>
                ▶ Play
              </Button>
              <Button variant="primary" click={() => sendPlaylist(true)}>
                Save
              </Button>
              <Button variant="default" click={() => setSaveOpen(true)}>
                Save as…
              </Button>
            </Footer>
          </Panel>
        </div>

        <div class={`split--stack ${view() === "library" || view() === "saved" ? "" : "hide-narrow"}`} style={{ "min-width": "0" }}>
          <div class={view() === "library" ? "" : "hide-narrow"}>
            <AnimationLibrary
              onAdd={(animation) => {
                mutate((items) => [...items, { animation, duration_ms: 10_000 }]);
                toast("Added to the playlist", "ok", 2_000);
              }}
            />
          </div>

          <div class={view() === "saved" ? "" : "hide-narrow"}>
            <Panel>
              <PanelHeader>
                Saved playlists
                <Spacer />
                <Button variant="ghost" size="icon" title="Refresh" click={() => refetchLibrary()}>
                  ⟳
                </Button>
              </PanelHeader>
              <Body>
                <Show when={library()} fallback={<span class="skeleton">Loading…</span>}>
                  {(entries) => (
                    <Show
                      when={entries().length > 0}
                      fallback={<div class="empty">Nothing saved yet — use “Save as…” to keep a show on the board.</div>}
                    >
                      <div class="list">
                        <For each={entries()}>
                          {(entry) => (
                            <div class="row">
                              <div class="row__main">
                                <span class="row__title">{entry.name}</span>
                                <span class="row__sub">{Math.round(entry.size / 10) / 100} KB</span>
                              </div>
                              <div class="row__actions">
                                <Button variant="cyan" click={() => loadLibraryItem(entry)}>
                                  Play
                                </Button>
                                <Button
                                  variant="default"
                                  title="Make this the board's startup playlist"
                                  click={() => setLibraryDefault(entry)}
                                >
                                  Set default
                                </Button>
                                <Show
                                  when={confirmDelete() === entry.name}
                                  fallback={
                                    <Button variant="danger" size="icon" title="Delete" click={() => setConfirmDelete(entry.name)}>
                                      🗑
                                    </Button>
                                  }
                                >
                                  <Button variant="danger" click={() => deleteLibraryItem(entry.name)}>
                                    Sure?
                                  </Button>
                                  <Button variant="ghost" click={() => setConfirmDelete(null)}>
                                    Keep
                                  </Button>
                                </Show>
                              </div>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  )}
                </Show>
              </Body>
            </Panel>

            <Show when={boardStatus()}>
              {(s) => (
                <Panel>
                  <PanelHeader>On the board now</PanelHeader>
                  <Body>
                    <div class="hstack" style={{ gap: "8px" }}>
                      <LedMatrix animation={s().now_playing.animation} fps={10} style={{ width: "140px" }} />
                      <div class="vstack" style={{ gap: "4px" }}>
                        <strong>{s().now_playing.title}</strong>
                        <span class="hint">
                          slot {s().playlist_index + 1}/{s().playlist_len}
                          {s().paused ? " · paused" : ""}
                          {s().status_screen ? " · status screen" : ""}
                        </span>
                      </div>
                    </div>
                  </Body>
                </Panel>
              )}
            </Show>
          </div>
        </div>
      </div>

      <Dialog
        open={saveOpen()}
        title="Save playlist to the board's library"
        onClose={() => setSaveOpen(false)}
        onConfirm={doSaveAs}
        confirmLabel="Save"
      >
        <div class="field">
          <label for="plname">Name</label>
          <input
            id="plname"
            class="input"
            placeholder="e.g. Lantern Parade v2"
            value={saveName()}
            onInput={(e) => setSaveName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                doSaveAs();
                setSaveOpen(false);
              }
            }}
          />
          <p class="hint">Letters, numbers, spaces, “-” and “_” only. The live playlist keeps playing either way.</p>
        </div>
      </Dialog>
    </div>
  );
}

function TabButton(props: { active: boolean; click: () => void; children: any }) {
  return (
    <button class={`btn ${props.active ? "btn--on" : "btn--ghost"}`} on:click={props.click}>
      {props.children}
    </button>
  );
}

// ── animation library ───────────────────────────────────────────────────────

function AnimationLibrary(props: { onAdd: (animation: Animation) => void }) {
  const [animations] = createResource(() => GlobalDeviceApi.getAnimationList());
  const [filter, setFilter] = createSignal("");

  const filtered = createMemo(() => {
    const list = animations() ?? [];
    const text = filter().toLowerCase().trim();

    if (!text) return list;

    return list.filter((a) => animationKind(a).toLowerCase().includes(text) || animationSummary(a).toLowerCase().includes(text));
  });

  return (
    <Panel accent="cyan">
      <PanelHeader>
        Animation library
        <Spacer />
        <input
          class="input"
          style={{ width: "160px", padding: "6px 10px" }}
          placeholder="Filter…"
          value={filter()}
          onInput={(e) => setFilter(e.currentTarget.value)}
        />
      </PanelHeader>
      <Body flush>
        <div class="lib-grid">
          <For each={filtered()}>{(animation) => <AnimCard animation={animation} onAdd={props.onAdd} />}</For>
        </div>
      </Body>
    </Panel>
  );
}

function AnimCard(props: { animation: Animation; onAdd: (animation: Animation) => void }) {
  // Local edited copy so users can tune the message/title before adding.
  const [current, setCurrent] = createSignal<Animation>(props.animation);

  const preview = createMemo(() => current());

  return (
    <div class="anim-card">
      <div class="anim-card__preview">
        <LedMatrix animation={preview()} fps={8} />
      </div>
      <div class="anim-card__body">
        <span class="anim-card__name">{humanise(animationKind(current()))}</span>
        <span class="anim-card__summary">{animationSummary(current()) || "—"}</span>
        <AnimParamsEditor animation={current()} onChange={setCurrent} />
        <div class="anim-card__actions">
          <Button
            variant="cyan"
            title="Preview on the board for 5 s"
            click={() => GlobalDeviceApi.send({ Animation: [current(), 5_000] }).catch(() => toast("Board offline", "error"))}
          >
            ▶
          </Button>
          <Button variant="primary" click={() => props.onAdd(current())}>
            + Add
          </Button>
        </div>
      </div>
    </div>
  );
}
