// Script de Orbital para la vista Visual de las notas .html (research R6 de la 002).
// Corre en un *world* aislado: comparte el DOM con la página, pero no su JavaScript (que además
// está apagado). Avisa los cambios a Orbital, aplica estilos como clases `o-…` y limpia el pegado.
(() => {
  // Reglas de las clases de Orbital; las pone Rust al cargar el script.
  const RULES = __RULES__;
  const post = (msg) => window.webkit.messageHandlers.orbital.postMessage(msg);
  document.addEventListener('input', changed, true);
  // Los marcadores recién escritos (o reinsertados) se envuelven con una pausa, para no
  // pelearse con el cursor en cada tecla.
  document.addEventListener('input', () => {
    clearTimeout(markerTimer);
    markerTimer = setTimeout(wrapPendingMarkers, 250);
  }, true);
  document.execCommand('styleWithCSS', false, false);

  // Los controles GTK y sus popovers están fuera del documento. Conservamos el
  // rango antes de devolverle el foco a WebKit para aplicar el formato allí.
  let savedRange = null;
  let selectionLocked = false;
  function rememberSelection() {
    const sel = getSelection();
    if (sel.rangeCount && document.body.contains(sel.getRangeAt(0).commonAncestorContainer)) {
      savedRange = sel.getRangeAt(0).cloneRange();
    }
  }
  function restoreSelection() {
    selectionLocked = false;
    if (!savedRange || !savedRange.commonAncestorContainer.isConnected) return;
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(savedRange);
  }
  document.addEventListener('selectionchange', () => {
    if (!selectionLocked) rememberSelection();
  });


  // Las reglas de las clases que se usan van en un único <style data-orbital> del <head>.
  function css() {
    const used = new Set();
    document.body.querySelectorAll('[class]').forEach((e) =>
      e.classList.forEach((c) => { if (RULES[c]) used.add(c); }));
    return Object.keys(RULES).filter((c) => used.has(c))
      .map((c) => '.' + c + ' { ' + RULES[c] + ' }').join('\n');
  }

  function changed() {
    let s = document.head.querySelector('style[data-orbital]');
    if (!s) {
      s = document.createElement('style');
      s.setAttribute('data-orbital', '');
      document.head.appendChild(s);
    }
    s.textContent = css();
    post('input');

    const sel = getSelection();
    if (sel.rangeCount && sel.isCollapsed) {
      const node = sel.anchorNode;
      if (node && node.nodeType === 3) {
        const text = node.textContent.slice(0, sel.anchorOffset);
        if (text.endsWith('[[')) {
          post('link-autocomplete');
        }
      }
    }
  }

  // El bloque de primer nivel donde está el cursor.
  function block() {
    const sel = getSelection();
    if (!sel.rangeCount) return null;
    let n = sel.anchorNode;
    if (n && n.nodeType === 3) n = n.parentNode;
    while (n && n.parentNode && n.parentNode !== document.body) n = n.parentNode;
    return n && n !== document.body && n.nodeType === 1 ? n : null;
  }

  function dropPrefix(el, prefix) {
    [...el.classList].filter((c) => c.startsWith(prefix)).forEach((c) => el.classList.remove(c));
    if (!el.classList.length) el.removeAttribute('class');
  }

  // Usar la pila de edición de WebKit también para clases y tablas: mutar el DOM
  // directamente no crea una operación deshacible.
  function replaceElement(original, edited) {
    const range = document.createRange();
    range.selectNode(original);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
    document.execCommand('insertHTML', false, edited.outerHTML);
    changed();
  }

  function blockClass(prefix, cls) {
    const b = block();
    if (!b) return;
    const edited = b.cloneNode(true);
    dropPrefix(edited, prefix);
    if (cls) edited.classList.add(cls);
    replaceElement(b, edited);
  }

  // Color de texto o de fondo sobre la selección: un <span class="o-…">.
  function spanClass(prefix, cls) {
    const sel = getSelection();
    if (!sel.rangeCount || sel.isCollapsed) return;
    const frag = sel.getRangeAt(0).cloneContents();
    frag.querySelectorAll('span[class]').forEach((s) => {
      dropPrefix(s, prefix);
      if (!s.attributes.length) s.replaceWith(...s.childNodes);
    });
    const wrapper = document.createElement('div');
    if (cls) {
      const span = document.createElement('span');
      span.className = cls;
      span.appendChild(frag);
      wrapper.appendChild(span);
    } else {
      wrapper.appendChild(frag);
    }
    document.execCommand('insertHTML', false, wrapper.innerHTML);
    changed();
  }

  function cell() {
    const sel = getSelection();
    let n = sel.rangeCount ? sel.anchorNode : null;
    if (n && n.nodeType === 3) n = n.parentNode;
    return n ? n.closest('td, th') : null;
  }

  function table(op) {
    if (op === 'insert') {
      const head = '<tr>' + '<th>Columna</th>'.repeat(3) + '</tr>';
      const row = '<tr>' + '<td><br></td>'.repeat(3) + '</tr>';
      document.execCommand('insertHTML', false,
        '<table><thead>' + head + '</thead><tbody>' + row + row + '</tbody></table><p><br></p>');
      return;
    }
    const c = cell();
    if (!c) return;
    const original = c.closest('table');
    const tbl = original.cloneNode(true);
    const tr = tbl.rows[c.parentNode.rowIndex];
    const idx = c.cellIndex;
    if (op === 'row-add') {
      const nr = document.createElement('tr');
      for (let k = 0; k < tr.children.length; k++) {
        const td = document.createElement('td');
        td.appendChild(document.createElement('br'));
        nr.appendChild(td);
      }
      if (tr.parentNode.tagName === 'THEAD') {
        const body = tbl.tBodies[0] || tbl.appendChild(document.createElement('tbody'));
        body.insertBefore(nr, body.firstChild);
      } else {
        tr.after(nr);
      }
    } else if (op === 'row-del') {
      if (tbl.rows.length > 1) tr.remove();
    } else if (op === 'col-add') {
      [...tbl.rows].forEach((r) => {
        const ref = r.children[idx];
        const n = document.createElement(ref && ref.tagName === 'TH' ? 'th' : 'td');
        n.appendChild(document.createElement('br'));
        if (ref) ref.after(n); else r.appendChild(n);
      });
    } else if (op === 'col-del') {
      if (tr.children.length > 1) [...tbl.rows].forEach((r) => r.children[idx] && r.children[idx].remove());
    }
    replaceElement(original, tbl);
  }

  function escapeAttr(s) {
    return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
  }

  // Lo pegado con formato se limpia: sin scripts, marcos, estilos en línea ni clases ajenas.
  function clean(html) {
    const doc = new DOMParser().parseFromString(html, 'text/html');
    doc.querySelectorAll('script, iframe, object, embed, style, link, meta, form, input, button')
      .forEach((e) => e.remove());
    // Un marcador pegado vuelve a su fuente: el span no sobrevive a la limpieza.
    doc.querySelectorAll('[data-orbital-marker]').forEach((el) =>
      el.replaceWith(doc.createTextNode('[%' + el.dataset.orbitalMarker + ']')));
    const allowed = new Set(['A', 'B', 'STRONG', 'I', 'EM', 'U', 'S', 'STRIKE', 'DEL',
      'P', 'H1', 'H2', 'H3', 'UL', 'OL', 'LI', 'BLOCKQUOTE', 'PRE', 'CODE', 'TABLE',
      'THEAD', 'TBODY', 'TFOOT', 'TR', 'TH', 'TD', 'IMG', 'HR', 'BR', 'DIV', 'SPAN']);
    doc.body.querySelectorAll('*').forEach((e) => {
      if (!allowed.has(e.tagName)) {
        e.replaceWith(...e.childNodes);
        return;
      }
      [...e.attributes].forEach((a) => {
        const n = a.name.toLowerCase();
        const keep = ['href', 'src', 'alt', 'title', 'colspan', 'rowspan'].includes(n)
          || (n === 'class' && [...e.classList].every((c) => RULES[c]));
        if (!keep || /^\s*javascript:/i.test(a.value)) e.removeAttribute(a.name);
      });
    });
    return doc.body.innerHTML;
  }

  document.addEventListener('paste', (e) => {
    const data = e.clipboardData;
    if (!data) return;
    const html = data.getData('text/html');
    e.preventDefault();
    if (html) document.execCommand('insertHTML', false, clean(html));
    else document.execCommand('insertText', false, data.getData('text/plain'));
  }, true);

  // Transclusiones `![[nota]]` (US5 de la 003): Orbital pasa el HTML de cada tarjeta y se inserta
  // después del bloque de primer nivel que contiene el `![[…]]`. Son elementos no editables con
  // `data-orbital-embed`: `body()` los quita, así nunca se guardan en el archivo.
  const EMBED_CSS =
    '.orbital-embed{border-left:3px solid #ff8cc6;background:rgba(255,140,198,0.07);' +
    'padding:0.4em 1em;margin:0.75em 0;border-radius:6px;user-select:text}' +
    '.orbital-embed-title{font-weight:600;margin-bottom:0.25em}' +
    '.orbital-embed-note{font-weight:normal;opacity:0.75}' +
    '.orbital-embed li.orbital-task{list-style:none;margin-left:-1.2em}';

  function setEmbeds(map) {
    document.querySelectorAll('[data-orbital-embed]').forEach((e) => e.remove());
    if (!document.getElementById('orbital-embed-style')) {
      const st = document.createElement('style');
      st.id = 'orbital-embed-style';
      st.textContent = EMBED_CSS;
      document.head.appendChild(st);
    }
    const keys = Object.keys(map);
    if (!keys.length) return;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const hits = [];
    let n;
    while ((n = walker.nextNode())) {
      for (const k of keys) if (n.data.includes(k)) hits.push([n, k]);
    }
    for (const [node, k] of hits) {
      let top = node.parentNode;
      while (top && top.parentNode && top.parentNode !== document.body) top = top.parentNode;
      if (!top || top === document.body) continue;
      const tpl = document.createElement('template');
      tpl.innerHTML = map[k];
      top.after(tpl.content);
    }
  }

  // Marcadores [%clave] (005): cada uno se envuelve en un span no editable con el valor
  // resuelto; body() los restituye a [%clave], así el disco siempre tiene el marcador.
  let markerMap = {};
  let markerTimer = null;

  // Envuelve los literales [%clave] que quedaron como texto (texto nuevo, un pegado o una
  // inserción). No toca los spans existentes: así el cursor no salta al escribir al lado.
  function wrapPendingMarkers() {
    const keys = Object.keys(markerMap);
    if (!keys.length) return;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (t) =>
        t.parentElement && t.parentElement.closest('[data-orbital-marker]')
          ? NodeFilter.FILTER_REJECT
          : NodeFilter.FILTER_ACCEPT,
    });
    const hits = [];
    let n;
    while ((n = walker.nextNode())) {
      for (const k of keys) {
        const pat = '[%' + k + ']';
        let at = 0;
        for (;;) {
          const found = n.data.indexOf(pat, at);
          if (found < 0) break;
          hits.push([n, found, k, pat.length]);
          at = found + pat.length;
        }
      }
    }
    // De atrás hacia adelante dentro de cada nodo: reemplazar no invalida los offsets previos.
    hits.sort((a, b) => b[1] - a[1]);
    for (const [node, at, key, len] of hits) {
      const tail = node.splitText(at); // texto desde el marcador
      const rest = tail.splitText(len); // texto después del marcador
      const span = document.createElement('span');
      span.setAttribute('contenteditable', 'false');
      span.setAttribute('data-orbital-marker', key);
      span.textContent = markerMap[key];
      tail.replaceWith(span);
      span.after(rest);
    }
  }

  function setMarkers(map) {
    markerMap = map;
    document.querySelectorAll('[data-orbital-marker]').forEach((el) =>
      el.replaceWith(document.createTextNode('[%' + el.dataset.orbitalMarker + ']')));
    wrapPendingMarkers();
  }

  // El cuerpo sin las tarjetas de transclusión; los marcadores vuelven a su fuente.
  function body() {
    const copy = document.body.cloneNode(true);
    copy.querySelectorAll('[data-orbital-embed]').forEach((e) => e.remove());
    copy.querySelectorAll('[data-orbital-marker]').forEach((el) =>
      el.replaceWith(document.createTextNode('[%' + el.dataset.orbitalMarker + ']')));
    return copy.innerHTML;
  }

  // Lo que llama Orbital desde la barra de formato.
  window.orbital = {
    setEmbeds,
    setMarkers,
    body,
    rememberSelection() {
      rememberSelection();
      selectionLocked = true;
    },
    restoreSelection,
    run(cmd, arg) {
      switch (cmd) {
        case 'color': return spanClass('o-color-', arg);
        case 'fondo': return spanClass('o-fondo-', arg);
        case 'alinear': return blockClass('o-alinear-', arg);
        case 'columnas': return blockClass('o-columnas-', arg);
        case 'estilo': return blockClass('o-estilo-', arg);
        case 'tabla': return table(arg);
        case 'imagen': {
          const [src, alt] = arg;
          document.execCommand('insertHTML', false,
            '<img src="' + escapeAttr(src) + '" alt="' + escapeAttr(alt) + '">');
          return;
        }
      }
    },
    // Lo que se guarda: `body()` (sin las tarjetas) y estas reglas (las de las clases usadas).
    css,
  };
})();
