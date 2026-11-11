import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { A } from "@solidjs/router";
import {
  animationKind,
  animationSummary,
  type BoardStatus,
  boardStatus,
  connected,
  formatDuration,
  formatUptime,
  GlobalDeviceApi,
  livePlaylist,
} from "@lib";
import { Body, Panel, PanelHeader, Spacer } from "../components/Panel.tsx";
import { Button } from "../components/Button.tsx";
import { LedMatrix } from "../components/LedMatrix.tsx";

export function NowRoute() {
  const status = boardStatus;

  const [joinStatus] = createResource(async () => {
    try {
      return await GlobalDeviceApi.getWifiJoinStatus();
    } catch {
      return null;
    }
  });

  return (
    <div class="page">
      <Panel accent="amber">
        <div class="panel__bd" style={{ gap: "18px" }}>
          <div class="hero">
            <div class="hero__matrix">
              <Show when={status()} fallback={<div class="led-matrix" style={{ "aspect-ratio": "3 / 2" }} />}>
                {(s) => <LedMatrix animation={s().now_playing.animation} fps={24} live />}
              </Show>
            </div>

            <div class="hero__meta">
              <NowPlaying status={status} />
              <Controls status={status} />
            </div>
          </div>
        </div>
      </Panel>

      <Show when={(livePlaylist()?.playlist.length ?? 0) > 1}>
        <Panel>
          <PanelHeader>
            Playlist
            <Spacer />
            <A class="chip" href="/playlists">
              Edit →
            </A>
          </PanelHeader>
          <Body>
            <div class="strip">
              <For each={livePlaylist()?.playlist ?? []}>
                {(item, i) => (
                  <button
                    class={`strip__item ${status()?.playlist_index === i() ? "strip__item--active" : ""}`}
                    title={`Play "${item.label ?? animationKind(item.animation)}" now`}
                    onClick={() => GlobalDeviceApi.send({ JumpTo: i() }).catch(() => {})}
                  >
                    <div class="strip__kind">{animationKind(item.animation)}</div>
                    <div class="strip__title">{item.label || animationSummary(item.animation) || animationKind(item.animation)}</div>
                    <div class="strip__dur">{formatDuration(item.duration_ms)}</div>
                  </button>
                )}
              </For>
            </div>
          </Body>
        </Panel>
      </Show>

      <div class="stat-cards">
        <Show when={status()}>
          {(s) => (
            <>
              <div class="stat">
                <span class="stat__label">Uptime</span>
                <span class="stat__value">{formatUptime(s().uptime_s)}</span>
              </div>
              <div class="stat">
                <span class="stat__label">Position</span>
                <span class="stat__value">
                  {s().playlist_len > 0 ? `${s().playlist_index + 1} / ${s().playlist_len}` : "—"}
                </span>
              </div>
            </>
          )}
        </Show>
        <div class="stat">
          <span class="stat__label">Feed</span>
          <span class="stat__value">
            <Show when={connected()} fallback={<span class="chip chip--red">offline</span>}>
              <span class="chip chip--green">live push</span>
            </Show>
          </span>
        </div>
        <Show when={joinStatus()}>
          {(j) => (
            <div class="stat">
              <span class="stat__label">Network</span>
              <span class="stat__value stat__value--mono truncate" title={j().ip ?? undefined}>
                {j().ap_fallback ? "AP fallback" : (j().ip ?? j().joined_ssid ?? "—")}
              </span>
            </div>
          )}
        </Show>
      </div>
    </div>
  );
}

function NowPlaying(props: { status: () => BoardStatus | null }) {
  const s = () => props.status();

  return (
    <div class="vstack" style={{ gap: "8px" }}>
      <div class="hstack" style={{ gap: "6px" }}>
        <span class="chip chip--amber">{s()?.now_playing.kind ?? "…"}</span>
        <Show when={s()?.paused}>
          <span class="chip">paused</span>
        </Show>
        <Show when={s()?.streaming}>
          <span class="chip chip--cyan">paint stream</span>
        </Show>
        <Show when={s()?.status_screen}>
          <span class="chip chip--red">status screen</span>
        </Show>
        <Show when={livePlaylist()} keyed>
          {(pl) => <span class="chip">{pl.source.toLowerCase()}</span>}
        </Show>
      </div>

      <div class="now-title">
        <h2>{s()?.now_playing.title ?? "Connecting to the board…"}</h2>
      </div>

      <div>
        <div class="progress">
          <div
            class="progress__fill"
            style={{ width: `${progressPct()}%`, transition: s()?.paused ? "none" : "width 1s linear" }}
          />
        </div>
        <div class="now-time">
          <span>{formatDuration(s()?.now_playing.elapsed_ms ?? 0)}</span>
          <span>{formatDuration(s()?.now_playing.duration_ms ?? 0)}</span>
        </div>
      </div>
    </div>
  );

  function progressPct() {
    const st = s();

    if (!st || st.now_playing.duration_ms <= 0) return 0;

    return Math.min(100, (st.now_playing.elapsed_ms / st.now_playing.duration_ms) * 100);
  }
}

function Controls(props: { status: () => BoardStatus | null }) {
  const s = () => props.status();

  const send = (msg: Parameters<typeof GlobalDeviceApi.send>[0]) => {
    GlobalDeviceApi.send(msg).catch((err) => console.warn("send failed:", err));
  };

  // While the user drags, the slider follows the finger instead of the
  // status echo; the override is dropped when the value is sent.
  const [dragPct, setDragPct] = createSignal<number | null>(null);

  const powerPct = () => dragPct() ?? s()?.power_limit_pct ?? 10;

  let powerTimer: ReturnType<typeof setTimeout> | null = null;

  onCleanup(() => {
    if (powerTimer) clearTimeout(powerTimer);
  });

  const setPower = (pct: number) => {
    setDragPct(pct);

    if (powerTimer) clearTimeout(powerTimer);

    powerTimer = setTimeout(() => {
      send({ PowerLimit: pct / 100 });
      setDragPct(null);
    }, 350);
  };

  return (
    <div class="vstack" style={{ gap: "14px" }}>
      <div class="btn-row" style={{ gap: "10px", "align-items": "center" }}>
        <Button
          class="big-toggle"
          variant={s()?.paused ? "primary" : "default"}
          aria-label={s()?.paused ? "Resume" : "Pause"}
          click={() => send({ Pause: !(s()?.paused ?? false) })}
        >
          {s()?.paused ? "▶" : "⏸"}
        </Button>

        <Button size="lg" variant="cyan" click={() => send("Next")}>
          ⏭ Next
        </Button>

        <Show when={s()?.streaming}>
          <Button variant="ghost" click={() => send("ResumeNow")}>
            ✕ stop painting
          </Button>
        </Show>
      </div>

      <div class="field">
        <label for="power">Power limit</label>
        <div class="slider-row">
          <input
            id="power"
            type="range"
            min="1"
            max="100"
            step="1"
            value={powerPct()}
            onInput={(e) => setPower(Number(e.currentTarget.value))}
          />
          <output>{powerPct()}%</output>
        </div>
      </div>

      <div class="btn-row">
        <For each={[5, 10, 25, 50, 100]}>
          {(p) => (
            <Button
              variant="ghost"
              on={s()?.power_limit_pct === p}
              style={{ padding: "5px 10px", "font-size": "12px" }}
              click={() => {
                setDragPct(null);
                send({ PowerLimit: p / 100 });
              }}
            >
              {p}%
            </Button>
          )}
        </For>
      </div>
    </div>
  );
}
