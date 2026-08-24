// OWA never raises notifications in this webview: its service worker
// disqualifies WebKitGTK upstream of permissions (no push service), never
// calling showNotification and leaving the notification store empty. The
// page DOM is the one reliable source, so this script drives everything:
// - writes the Inbox unread count into the tab title as "(N) " (OWA stopped
//   titling the count; reproducible in Firefox), feeding the native title
//   watcher that drives the tray badge
// - on a count increase, raises a Notification per new unread row (sender +
//   subject), falling back to a generic one when no new row is identifiable;
//   the constructor is the one notification path WebKitGTK delivers to the
//   embedder, where the native side forwards it over DBus
// ponytail: anchored to English OWA labels ("Inbox", "Unread"); other display
// languages degrade to a bare title and no notifications.
(() => {
  const readUnread = () => {
    for (const el of document.querySelectorAll('[role="treeitem"]')) {
      // textContent concatenates the badge without separators ("Inbox53unread"),
      // so no word-boundary matching. Containment, not startsWith: the labels
      // carry invisible characters that survive trim() and defeat prefix
      // matching while printing identically.
      const label = (el.getAttribute("aria-label") || el.textContent || "").trim();
      if (label.includes("Inbox")) {
        const m = label.match(/(\d+)\s*unread/);
        return m ? parseInt(m[1], 10) : 0;
      }
    }
    return null; // folder pane not rendered yet
  };

  const unreadRows = () =>
    [...document.querySelectorAll('[role="option"][data-convid]')].filter((row) =>
      (row.getAttribute("aria-label") || "").includes("Unread")
    );

  // Sender lives in a span whose title attribute is the address; the hashed
  // class names are not usable anchors. The aria-label reads
  // "Unread [External sender] <name> <subject> <HH:MM> <body preview>", so
  // subject is the text between the sender's display name and the time stamp,
  // and the preview is everything after it (minus concatenated status junk
  // like "No items selected" on the selected row).
  const extract = (row) => {
    const senderEl = row.querySelector('span[title*="@"]');
    const sender = senderEl ? senderEl.textContent.trim() : null;
    let subject = null;
    let preview = null;
    const label = row.getAttribute("aria-label") || "";
    if (sender && label.includes(sender)) {
      const after = label.slice(label.indexOf(sender) + sender.length);
      const m = after.match(/^\s*(.*?)\s*\d{1,2}[:.]\d{2}\s*([\s\S]*)$/);
      if (m) {
        if (m[1]) subject = m[1];
        const p = m[2].replace(/No items selected\s*$/, "").trim();
        if (p) preview = p;
      }
    }
    return { sender, subject, preview };
  };

  const fire = (title, body) => {
    try {
      new Notification(title, { body });
    } catch (e) {
      console.warn("[unread-title] notification failed:", e.message);
    }
  };

  const seenIds = new Set();
  let primed = false;
  let lastUnread = null;

  setInterval(() => {
    const unread = readUnread();
    if (unread === null) return;

    const base = document.title.replace(/^\(\d+\)\s*/, "");
    const wanted = `(${unread}) ${base}`;
    if (document.title !== wanted) document.title = wanted;

    const rows = unreadRows();

    if (!primed) {
      // First reading is the baseline: existing unread mail is not news.
      for (const row of rows) seenIds.add(row.getAttribute("data-convid"));
      primed = true;
      lastUnread = unread;
      return;
    }

    if (unread > lastUnread) {
      const fresh = rows.filter((row) => !seenIds.has(row.getAttribute("data-convid")));
      if (fresh.length > 0) {
        for (const row of fresh.slice(0, 3)) {
          const { sender, subject, preview } = extract(row);
          const body = [subject, preview].filter(Boolean).join("\n");
          fire(sender || "New email", body || `${unread} unread emails`);
        }
        if (fresh.length > 3) {
          fire("New email", `${fresh.length} new emails, ${unread} unread`);
        }
      } else {
        // A reply landing in an already-seen conversation adds no new row id.
        fire("New email", `${unread} unread emails`);
      }
    }

    for (const row of rows) seenIds.add(row.getAttribute("data-convid"));
    lastUnread = unread;
  }, 5000);
})();
