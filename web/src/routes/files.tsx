import { Button, Card } from "@components";
import { downloadFile, GlobalDeviceApi, uploadFile } from "@lib";
import { useNavigate } from "@solidjs/router";
import { createSignal } from "solid-js";
import { createResource, For, Show } from "solid-js";
import { Suspense } from "solid-js/web";

export function FilesRoute() {
  const navigate = useNavigate();
  const api = GlobalDeviceApi;

  const [files, { refetch }] = createResource(() => api.listFiles());
  const [selected, setSelected] = createSignal<string | null>(null);

  const onFileClick = (filename: string) => {
    setSelected(filename);
  };

  const onUploadFile = async () => {
    const [filename, data] = await uploadFile();

    await api.writeFile(filename, data);
    await refetch();
  };

  const onEmulatorFile = () => {
    navigate(`/emulator/${selected()}`);
  };

  const onDownloadFile = async () => {
    const bytes = await api.readFile(selected()!);

    downloadFile(selected()!, bytes);
  };

  const onDeleteFile = async () => {
    await api.deleteFile(selected()!);
    await refetch();
  };

  return (
    <div class="FilesRoute">
      <Card colour="warning">
        <Card.Header text="Files" />
        <Card.Body pad={0}>
          <Suspense>
            <Show when={files()}>
              {(files) => (
                <div class="Files d-flex flex-column">
                  <For each={files()}>
                    {(file) => (
                      <div
                        class="Files__file p-3 grid align-items-center"
                        classList={{ selected: file.name === selected() }}
                        on:click={() => onFileClick(file.name)}
                      >
                        <div class="Files__filename g-col-12 g-col-lg-4">{file.name}</div>
                        <div class="Files__filesize g-col-12 g-col-lg-4">{file.size} bytes</div>

                        <Show when={file.name === selected()}>
                          <div class="g-col-12 d-flex gap-1">
                            <Button colour="danger" on:click={() => onDeleteFile()}>Delete</Button>
                            <Button colour="secondary" on:click={() => onDownloadFile()}>Download</Button>
                            <Button colour="primary" on:click={() => onEmulatorFile()}>Emulate</Button>
                          </div>
                        </Show>
                      </div>
                    )}
                  </For>
                </div>
              )}
            </Show>
          </Suspense>
        </Card.Body>
        <Card.Footer>
          <Button colour="info" on:click={() => onUploadFile()}>Upload</Button>
        </Card.Footer>
      </Card>
    </div>
  );
}
