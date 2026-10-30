// Fills in a note on /new the way a person would, for a screenshot run in the
// simulator: the title, the place from the app, a photo (drawn here, since the
// simulator has no camera to pick from), two seconds of the microphone, then save.
// mobile/ios/build.sh mobile/examples/fieldbook, then:
//   xcrun simctl launch booted org.quillolang.app --eval "location.href='/new'" \
//     --eval "$(cat mobile/examples/fieldbook/automation/capture.js)"
(async () => {
  const pause = (ms) => new Promise((r) => setTimeout(r, ms));
  const button = (text) => [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const title = document.querySelector("input[placeholder='What is it?']");
  title.value = "Rain tree by the river";
  title.dispatchEvent(new Event("input", { bubbles: true }));
  const body = document.querySelector("textarea");
  body.value = "Old Samanea saman, crown about 25 m across. Leaves folding before the rain.";
  body.dispatchEvent(new Event("input", { bubbles: true }));
  button("Locate (app)").click();
  await pause(1500);
  // A photo: a sky, a trunk and a crown, as a JPEG file in the input.
  const c = document.createElement("canvas");
  c.width = 1600; c.height = 1200;
  const g = c.getContext("2d");
  const sky = g.createLinearGradient(0, 0, 0, 1200);
  sky.addColorStop(0, "#7fb3d5"); sky.addColorStop(1, "#d6eaf8");
  g.fillStyle = sky; g.fillRect(0, 0, 1600, 1200);
  g.fillStyle = "#6e8b3d"; g.fillRect(0, 950, 1600, 250);
  g.fillStyle = "#5d4037"; g.fillRect(740, 560, 120, 420);
  g.fillStyle = "#2e7d32";
  for (const [x, y, r] of [[800, 470, 330], [520, 560, 220], [1080, 560, 220], [800, 300, 200]]) { g.beginPath(); g.arc(x, y, r, 0, Math.PI * 2); g.fill(); }
  const blob = await new Promise((r) => c.toBlob(r, "image/jpeg", 0.9));
  const dt = new DataTransfer();
  dt.items.add(new File([blob], "rain-tree.jpg", { type: "image/jpeg" }));
  const file = document.querySelector("input[type=file]");
  file.files = dt.files;
  file.dispatchEvent(new Event("change", { bubbles: true }));
  button("Record").click();
  await pause(2500);
  button("Stop").click();
  await pause(1500);
  window.scrollTo(0, document.body.scrollHeight);
  await pause(6000); // time for a screenshot of the filled-in form
  button("Save the note").click();
})();
