// Plain JavaScript beside the components (ui/page.qwe loads it). Components
// call into it by name: js("pm.focus", id).

// Quillo has no time zones: tell the server ours (minutes east of UTC), so
// "today" and "overdue" are the viewer's. The first page a browser ever
// asks for is made before the cookie exists, in UTC.
(function () {
  var tz = -new Date().getTimezoneOffset();
  if (!new RegExp("(?:^|; )tz=" + tz + "(?:;|$)").test(document.cookie)) {
    document.cookie = "tz=" + tz + "; path=/; max-age=31536000; samesite=lax";
  }
})();

window.pm = {
  // After a card moves with the keyboard: keep the focus on it.
  focus(id) {
    requestAnimationFrame(() => {
      const el = document.getElementById(id);
      if (el) el.focus();
    });
  },
  go(path) {
    location.assign(path);
  },
  // Whether one of Quillo's ask() dialogs is showing: Esc is its, then.
  dialogOpen() {
    return !!document.querySelector("dialog[open], [role=alertdialog]");
  },
};
