// Keep this text as it is: it is named by its hash in kept pages' content policies, so a change
// leaves the copies kept before it hidden (failing closed) until the new version has kept its own.
(()=>{const h=document.documentElement,t=h.getAttribute("data-kept-session"),m=/(?:^|;\s*)quillo_tag=([^;]*)/.exec(document.cookie);if((m?m[1]:"-")===t)return h.removeAttribute("hidden");stop();const c=navigator.serviceWorker&&navigator.serviceWorker.controller;if(!c)return;const p=new MessageChannel;p.port1.onmessage=()=>location.reload();c.postMessage({quillo:"drop",who:t},[p.port2])})()
