import { GlobalDeviceApi } from "@lib";
import { RouteSectionProps } from "@solidjs/router";

export function RemoteRoute(props: RouteSectionProps) {
  const api = GlobalDeviceApi;

  // api.getAnimationInfo();

  return (
    <div class="grid">
      TODO Select
    </div>
  );
}
