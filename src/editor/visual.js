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
  // Una fórmula dibujada se edita como texto: un clic la vuelve a su fuente (050).
  document.addEventListener('click', (e) => {
    const f = e.target.closest && e.target.closest('[data-orbital-formula]');
    if (!f) return;
    const t = document.createTextNode(f.dataset.orbitalFormula);
    f.replaceWith(t);
    const r = document.createRange();
    r.setStart(t, Math.min(1, t.length));
    r.collapse(true);
    const sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    editingFormula = t;
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
    doc.querySelectorAll('[data-orbital-formula]').forEach((el) =>
      el.replaceWith(doc.createTextNode(el.dataset.orbitalFormula)));
    // Un gráfico pegado: la figura sale y su fuente queda en el `pre`.
    doc.querySelectorAll('[data-orbital-grafico]').forEach((el) => el.remove());
    doc.querySelectorAll('[data-orbital-grafico-oculto]').forEach((el) =>
      el.removeAttribute('data-orbital-grafico-oculto'));
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

  // Fórmulas (050): Orbital pasa, para cada fórmula tal como se ve en el texto (`$x^2$`), su
  // SVG. Cada una se envuelve en un span no editable con el fuente en `data-orbital-formula`;
  // body() y el pegado la restituyen, así el disco solo tiene el LaTeX.
  const FORMULA_CSS =
    '.orbital-formula{display:inline-block;cursor:text}' +
    '.orbital-formula-bloque{display:block;text-align:center;margin:0.4em 0}' +
    '.orbital-formula-error{font-family:monospace;text-decoration:underline wavy #ff8cc6}';
  const RAW = 'code, pre, script, style, textarea, title, svg, math, [data-orbital-formula]';
  let formulaMap = {};
  let editingFormula = null;

  function wrapPendingFormulas() {
    const keys = Object.keys(formulaMap).sort((a, b) => b.length - a.length);
    if (!keys.length) return;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (t) =>
        !t.data.includes('$') || (t.parentElement && t.parentElement.closest(RAW)) ||
        t === editingFormula
          ? NodeFilter.FILTER_REJECT
          : NodeFilter.FILTER_ACCEPT,
    });
    const nodes = [];
    let n;
    while ((n = walker.nextNode())) nodes.push(n);
    for (const node of nodes) {
      const hits = [];
      for (const k of keys) {
        let at = 0;
        for (;;) {
          const found = node.data.indexOf(k, at);
          if (found < 0) break;
          const end = found + k.length;
          const inline = !k.startsWith('$$');
          const overlaps = hits.some(([a, len]) => found < a + len && a < end);
          if (!overlaps && !(inline && /[0-9]/.test(node.data.charAt(end)))) {
            hits.push([found, k.length, k]);
          }
          at = end;
        }
      }
      hits.sort((a, b) => b[0] - a[0]);
      for (const [at, len, key] of hits) {
        const tail = node.splitText(at);
        const rest = tail.splitText(len);
        const span = document.createElement('span');
        const html = formulaMap[key];
        const error = html.startsWith('!');
        span.setAttribute('contenteditable', 'false');
        span.setAttribute('data-orbital-formula', key);
        span.className = 'orbital-formula' + (error ? ' orbital-formula-error' : '') +
          (!error && key.startsWith('$$') ? ' orbital-formula-bloque' : '');
        if (error) {
          span.title = html.slice(1);
          span.textContent = key;
        } else {
          span.innerHTML = html;
        }
        tail.replaceWith(span);
        span.after(rest);
      }
    }
  }

  function setFormulas(map) {
    formulaMap = map;
    if (!document.getElementById('orbital-formula-style')) {
      const st = document.createElement('style');
      st.id = 'orbital-formula-style';
      st.textContent = FORMULA_CSS;
      document.head.appendChild(st);
    }
    // Las que ya no están (se borraron o cambiaron en Código) vuelven a texto.
    document.querySelectorAll('[data-orbital-formula]').forEach((el) => {
      if (!(el.dataset.orbitalFormula in map)) {
        el.replaceWith(document.createTextNode(el.dataset.orbitalFormula));
      }
    });
    // La que se está editando se vuelve a dibujar cuando el cursor sale de ella.
    const sel = window.getSelection();
    if (editingFormula && !(sel.anchorNode === editingFormula)) editingFormula = null;
    wrapPendingFormulas();
  }

  // Gráficos (052) y diagramas Mermaid (058): para cada `pre > code.language-grafico` (o
  // `language-mermaid`), Orbital pasa su SVG (estático) por el fuente, o `!` y el motivo. La
  // figura va después del `pre`, que se oculta; no es editable y body() la quita. Un clic en la
  // figura muestra el fuente. Cada tipo lleva su `data-orbital-tipo`: poner los de uno no saca
  // los del otro.
  const GRAFICO_CSS =
    'pre[data-orbital-grafico-oculto]{display:none}' +
    '.orbital-grafico{margin:0.6em 0;cursor:pointer;max-width:100%}' +
    '.orbital-grafico svg{max-width:100%;height:auto}' +
    '.orbital-grafico-error{color:#ff8cc6;cursor:default}';
  const FIGURAS = {
    grafico: { titulo: 'Clic para editar el fuente del gráfico' },
    mermaid: { titulo: 'Clic para editar el fuente del diagrama' },
  };
  function ponerFiguras(map, tipo) {
    if (!document.getElementById('orbital-grafico-style')) {
      const st = document.createElement('style');
      st.id = 'orbital-grafico-style';
      st.textContent = GRAFICO_CSS;
      document.head.appendChild(st);
    }
    document.querySelectorAll('[data-orbital-grafico][data-orbital-tipo="' + tipo + '"]')
      .forEach((f) => {
        const pre = f.previousElementSibling;
        if (pre) pre.removeAttribute('data-orbital-grafico-oculto');
        f.remove();
      });
    document.querySelectorAll('pre > code.language-' + tipo).forEach((code) => {
      const pre = code.parentElement;
      if (pre.closest('[data-orbital-embed]')) return;
      pre.removeAttribute('data-orbital-grafico-oculto');
      // El que se está editando queda como fuente hasta que el cursor salga.
      const sel = window.getSelection();
      if (sel && sel.anchorNode && pre.contains(sel.anchorNode)) return;
      const src = code.textContent;
      let html = map[src];
      if (html === undefined) html = map[src.replace(/\n$/, '')];
      if (html === undefined) return;
      const fig = document.createElement('figure');
      fig.setAttribute('contenteditable', 'false');
      fig.setAttribute('data-orbital-grafico', '');
      fig.setAttribute('data-orbital-tipo', tipo);
      if (html.startsWith('!')) {
        fig.className = 'orbital-grafico orbital-grafico-error';
        fig.setAttribute('role', 'alert');
        fig.textContent = '⚠ ' + html.slice(1);
        pre.after(fig);
        return;
      }
      fig.className = 'orbital-grafico';
      fig.title = FIGURAS[tipo].titulo;
      fig.innerHTML = html;
      fig.addEventListener('click', () => {
        pre.removeAttribute('data-orbital-grafico-oculto');
        fig.remove();
      });
      pre.setAttribute('data-orbital-grafico-oculto', '');
      pre.after(fig);
    });
  }
  function setGraficos(map) {
    ponerFiguras(map, 'grafico');
  }
  function setDiagramas(map) {
    ponerFiguras(map, 'mermaid');
  }

  // Ids de bloque (026): cada `p`, `li`, `blockquote`, `table`, `pre` y `figure` sin `id` recibe uno
  // (6 caracteres `a-z0-9`, como en Código). Enter en un bloque con id copia el atributo al bloque
  // nuevo: los repetidos se reemplazan desde el segundo. El atributo no se ve, así que también lo
  // recibe el bloque que se está escribiendo.
  const ID_CHARS = 'abcdefghijklmnopqrstuvwxyz0123456789';
  function newId(used) {
    const bytes = new Uint8Array(6);
    for (;;) {
      crypto.getRandomValues(bytes);
      let id = '';
      for (const b of bytes) id += ID_CHARS[b % 36];
      if (!used.has(id)) {
        used.add(id);
        return id;
      }
    }
  }
  function assignBlockIds() {
    const used = new Set();
    document.body.querySelectorAll('[id]').forEach((e) => used.add(e.id));
    const seen = new Set();
    document.body.querySelectorAll('p, li, blockquote, table, pre, figure').forEach((e) => {
      if (e.closest('[data-orbital-embed]')) return;
      if (e.id && !seen.has(e.id)) {
        seen.add(e.id);
        return;
      }
      e.id = newId(used);
      seen.add(e.id);
    });
  }

  // El cuerpo sin las tarjetas de transclusión; los marcadores vuelven a su fuente.
  function body() {
    assignBlockIds();
    const copy = document.body.cloneNode(true);
    copy.querySelectorAll('[data-orbital-embed]').forEach((e) => e.remove());
    copy.querySelectorAll('[data-orbital-marker]').forEach((el) =>
      el.replaceWith(document.createTextNode('[%' + el.dataset.orbitalMarker + ']')));
    copy.querySelectorAll('[data-orbital-formula]').forEach((el) =>
      el.replaceWith(document.createTextNode(el.dataset.orbitalFormula)));
    copy.querySelectorAll('#orbital-formula-style').forEach((e) => e.remove());
    copy.querySelectorAll('[data-orbital-grafico]').forEach((e) => e.remove());
    copy.querySelectorAll('[data-orbital-grafico-oculto]').forEach((e) =>
      e.removeAttribute('data-orbital-grafico-oculto'));
    copy.querySelectorAll('#orbital-grafico-style').forEach((e) => e.remove());
    return copy.innerHTML;
  }

  // Lo que llama Orbital desde la barra de formato.
  window.orbital = {
    setEmbeds,
    setMarkers,
    setFormulas,
    setGraficos,
    setDiagramas,
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
