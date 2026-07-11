import { isNotNull, uploadFile } from "@lib";
import "./style.scss";

interface Props {
  onFile: (buffer: Uint8Array) => void;
}

export function DropZone(props: Props) {
  const onClickUploadFile = async () => {
    const [, buffer] = await uploadFile();
    props.onFile(buffer);
  };

  const onDragOver = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();

    (e.target as HTMLDivElement).classList.add("dragging");
  };

  const onDrop = async (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();

    (e.target as HTMLDivElement).classList.remove("dragging");

    const files = (e.dataTransfer ? [...e.dataTransfer.items] : [])
      .map((item) => item.getAsFile())
      .filter(isNotNull);

    if (files.length > 0) {
      props.onFile(new Uint8Array(await files[0].arrayBuffer()));
    }
  };

  return (
    <div
      class="DropZone"
      on:click={onClickUploadFile}
      on:dragover={onDragOver}
      on:drop={onDrop}
    >
      Drag file here or click to upload
    </div>
  );
}
