/** Whether the command palette (Ctrl+K) is open */
class PaletteStore {
  open = $state(false);

  toggle() {
    this.open = !this.open;
  }
}

export const palette = new PaletteStore();
