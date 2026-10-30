// A module that takes its time, told by its name (tests/browser/modules.qwe).
await new Promise((r) => setTimeout(r, 250));
export const late = () => "late";
