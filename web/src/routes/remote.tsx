import { Button, Card } from "@components";
import { Animation, AnimationParams, assertError, assertUnreachable, GlobalDeviceApi, openAlert, Playlist } from "@lib";
import { RouteSectionProps } from "@solidjs/router";
import { createEffect, createResource, createSignal, For } from "solid-js";

export function RemoteRoute(props: RouteSectionProps) {
  const api = GlobalDeviceApi;

  const [animations, {}] = createResource(async () => {
    try {
      return await api.getAnimationList();
    } catch (err: unknown) {
      assertError(err);
      console.error(err);
      openAlert("Error", err.message);
      return [];
    }
  });
  const [playlist, { mutate }] = createResource<Playlist>(() => api.getPlaylist());

  const onAddAnimation = async (animation: Animation) => {
    if (!playlist.latest) return;
    mutate([...playlist.latest, [animation, 10_000]]);
  };

  const onPreview = async (animation: Animation) => {
    api.sendMessage({ Animation: [animation, 5_000] });
  };

  const onUpdatePlaylistAnimation = async (idx: number, animation: Animation) => {
    if (!playlist.latest) return;

    if (JSON.stringify(playlist.latest[idx][0]) !== JSON.stringify(animation)) {
      const playlist_clone = structuredClone(playlist.latest);

      playlist_clone[idx] = [animation, playlist_clone[idx][1]];

      mutate(playlist_clone);
    }
  };

  const onUpdatePlaylistAnimationDuration = async (idx: number, duration: number) => {
    if (!playlist.latest) return;

    const playlist_clone = structuredClone(playlist.latest);

    playlist_clone[idx] = [playlist_clone[idx][0], duration];

    mutate(playlist_clone);
  };

  const onDeletePlaylistItem = async (idx: number) => {
    if (!playlist.latest) return;

    mutate(playlist.latest.filter((_, i) => i != idx));
  };

  const onRun = async () => {
    if (!playlist.latest) return;

    await api.sendMessage({ Playlist: { playlist: playlist.latest, save: false } });
  };

  const onSave = async () => {
    if (!playlist.latest) return;

    await api.sendMessage({ Playlist: { playlist: playlist.latest, save: true } });
  };

  const onReset = async () => {
    await api.sendMessage("Reset");
  };

  return (
    <div class="grid">
      <div class="g-col-12 g-col-md-6">
        <Card colour="warning">
          <Card.Header text="Animation Library" />
          <Card.Body>
            <div class="d-flex flex-column gap-2">
              <For each={animations()}>
                {(animation) => (
                  <div>
                    {/* {JSON.stringify(animation)} */}
                    <AnimationForm animation={animation} onSave={onAddAnimation} onPreview={onPreview} />
                  </div>
                )}
              </For>
            </div>
          </Card.Body>
        </Card>
      </div>
      <div class="g-col-12 g-col-md-6">
        <Card colour="info">
          <Card.Header text="Playlist" />
          <Card.Body>
            <div class="d-flex flex-column gap-2">
              <For each={playlist()}>
                {(animation, idx) => (
                  <div class="p-2 d-flex flex-column gap-2 border border-info rounded-2 bg-info-subtle">
                    <AnimationForm animation={animation[0]} onUpdate={(a) => onUpdatePlaylistAnimation(idx(), a)} />
                    <div class="d-flex gap-2 align-items-center">
                      <span>Duration:</span>
                      <input
                        class="form-control"
                        type="number"
                        value={animation[1]}
                        on:change={(e) => onUpdatePlaylistAnimationDuration(idx(), Number(e.currentTarget.value))}
                      />
                      <Button colour="danger" on:click={() => onDeletePlaylistItem(idx())}>Delete</Button>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Card.Body>
          <Card.Footer>
            <Button colour="info" on:click={() => onRun()}>Run</Button>
            <Button colour="primary" on:click={() => onSave()}>Save</Button>
            <Button colour="danger" on:click={() => onReset()}>Reset</Button>
          </Card.Footer>
        </Card>
      </div>
    </div>
  );
}

interface Props {
  animation: Animation;
  onSave?: (animation: Animation) => Promise<void>;
  onUpdate?: (animation: Animation) => Promise<void>;
  onPreview?: (animation: Animation) => Promise<void>;
}

export function AnimationForm(props: Props) {
  const [animation, setAnimation] = createSignal<Animation>(props.animation);

  // Handle primitive value updates (string, number, boolean)
  const primitiveForm = (value: string | number | boolean, path: string[]) => {
    const updateValue = (newValue: any) => {
      setAnimation((prev) => {
        // Deep update using path
        const clone = structuredClone(prev);
        let current: any = clone;
        for (let i = 0; i < path.length - 1; i++) {
          current = current[path[i]];
        }
        current[path[path.length - 1]] = newValue;
        return clone;
      });
    };

    if (typeof value === "string") {
      return (
        <input
          class="form-control"
          type="text"
          value={value}
          on:change={(e) => updateValue(e.currentTarget.value)}
        />
      );
    } else if (typeof value === "number") {
      return (
        <input
          class="form-control"
          type="number"
          value={value}
          on:change={(e) => updateValue(Number(e.currentTarget.value))}
        />
      );
    } else if (typeof value === "boolean") {
      return (
        <input
          type="checkbox"
          checked={value}
          on:change={(e) => updateValue(e.currentTarget.checked)}
        />
      );
    } else {
      assertUnreachable(value);
    }
  };

  // Handle params (array or object)
  const paramsForm = (params: AnimationParams, path: string[] = []) => {
    if (typeof params === "string" || typeof params === "number" || typeof params === "boolean") {
      return (
        <div class="g-col-12">
          {primitiveForm(params, path)}
        </div>
      );
    } else if (Array.isArray(params)) {
      return (
        <div class="grid gap-2">
          {params.map((param, index) => paramsForm(param, [...path, index.toString()]))}
        </div>
      );
    } else if (typeof params === "object" && params !== null) {
      return (
        <div>
          {Object.entries(params).map(([key, value]) => (
            <div class="mb-2">
              <label class="font-medium">{key}:</label>
              {paramsForm(value, [...path, key])}
            </div>
          ))}
        </div>
      );
    } else {
      assertUnreachable(params);
    }
  };

  // Render the full form
  const form = () => {
    const _animation = animation();

    if (typeof _animation === "string") {
      // Handle top-level primitive (unlikely but possible)
      return (
        <h3 class="mb-0">
          {_animation}
        </h3>
      );
    } else if (typeof _animation === "object" && _animation !== null) {
      const keys = Object.keys(_animation);
      if (keys.length === 0) {
        return <div>No animation properties</div>;
      }

      return (
        <div>
          {keys.map((animationName) => {
            const params = _animation[animationName];
            return (
              <div class="d-flex flex-column gap-2">
                <h3 class="mb-0">{animationName}</h3>
                {paramsForm(params, [animationName])}
              </div>
            );
          })}
        </div>
      );
    } else {
      assertUnreachable(_animation);
    }
  };

  if (props.onUpdate) {
    createEffect(() => {
      props.onUpdate!(animation());
    });
  }

  return (
    <div class="p-2 d-flex flex-column gap-2 border border-primary rounded-2 bg-primary-subtle">
      {form()}
      {props.onSave && props.onPreview && (
        <div class="d-flex gap-2 flex-row-reverse">
          {props.onSave && <Button colour="primary" on:click={async () => await props.onSave!(animation())}>Add</Button>}
          {props.onPreview && <Button colour="info" on:click={async () => await props.onPreview!(animation())}>Preview</Button>}
        </div>
      )}
    </div>
  );
}
