export class TimerRegister {
  private timers: number[] = [];

  public setTimer(ms: number): number {
    const id = this.timers.push(0) - 1;

    setTimeout(() => {
      this.timers[id] = 1;
    }, ms);

    return id;
  }

  public checkTimer(id: number): number {
    const expired = this.timers[id];

    if (expired) delete this.timers[id];

    return expired;
  }
}
