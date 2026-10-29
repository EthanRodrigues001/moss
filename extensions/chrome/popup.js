// Moss Connector: reads the signed-in Overleaf cookies from this Chrome profile and hands them
// to the Moss desktop app listening on this PC (127.0.0.1 only). Nothing is sent anywhere else.

const PORTS = [47831, 47832, 47833, 47834, 47835];
const codeInput = document.getElementById("code");
const sendButton = document.getElementById("send");
const statusEl = document.getElementById("status");

function show(text, ok) {
  statusEl.textContent = text;
  statusEl.className = ok ? "ok" : "bad";
}

async function findMoss() {
  for (const port of PORTS) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/moss/ping`, {
        signal: AbortSignal.timeout(800),
      });
      if (res.ok && (await res.json()).moss) return port;
    } catch {
      // try the next port
    }
  }
  return null;
}

async function send() {
  const code = codeInput.value.trim();
  if (!/^\d{6}$/.test(code)) return show("Enter the 6-digit code shown in Moss.", false);

  sendButton.disabled = true;
  show("Working…", true);
  try {
    const cookies = await chrome.cookies.getAll({ url: "https://www.overleaf.com/" });
    if (!cookies.some((c) => c.name.startsWith("overleaf_session"))) {
      return show("You are not signed in to Overleaf in this Chrome profile.", false);
    }
    const port = await findMoss();
    if (!port) return show("Moss is not waiting. Click Connect from Chrome in Moss first.", false);

    const res = await fetch(`http://127.0.0.1:${port}/moss/overleaf-session`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        code,
        cookies: cookies.map((c) => ({ name: c.name, value: c.value })),
      }),
    });
    const body = await res.json().catch(() => ({}));
    if (res.ok && body.ok) show(`Connected${body.email ? ` as ${body.email}` : ""}. You can close this.`, true);
    else show(body.error === "wrong code" ? "Wrong code. Check Moss and try again." : body.error || "Moss rejected it.", false);
  } catch (e) {
    show(`Failed: ${e.message}`, false);
  } finally {
    sendButton.disabled = false;
  }
}

sendButton.addEventListener("click", send);
codeInput.addEventListener("keydown", (e) => e.key === "Enter" && send());
