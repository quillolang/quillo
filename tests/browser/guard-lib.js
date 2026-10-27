// A stand-in for a library that hands the program more than it should.
window.lib = {
  R: Reflect,
  parser: new DOMParser(),
  parsed: (html) => new DOMParser().parseFromString(html, "text/html"),
};
