(() => {
  'use strict';
  const themes = [
    { id: 'original', name: 'Original', note: 'Ljubičasta i plavi sjaj' },
    { id: 'kobalt', name: 'Kobalt', note: 'Duboka ponoćna plava' },
    { id: 'ametist', name: 'Ametist', note: 'Mirni ljubičasti tonovi' },
    { id: 'sljiva', name: 'Šljiva', note: 'Topli magenta odsjaj' },
    { id: 'petrol', name: 'Petrol', note: 'Plava sa tirkiznim sjajem' },
    { id: 'grafit', name: 'Grafit', note: 'Prigušena siva i plava' }
  ];
  const sections = [
    { id: 'background', name: 'Pozadina', parameter: 'theme', attribute: 'data-background-theme' },
    { id: 'table', name: 'Tabele i kartice', parameter: 'table', attribute: 'data-table-theme' }
  ];
  const knownTheme = id => themes.find(theme => theme.id === id) || themes[0];
  const fromURL = parameter => {
    try { return knownTheme(new URL(window.location.href).searchParams.get(parameter)); }
    catch (_) { return themes[0]; }
  };
  const current = {};
  let activeSection = 'background';
  const setAppearance = (section, theme) => {
    current[section.id] = theme;
    if (theme.id === 'original') document.documentElement.removeAttribute(section.attribute);
    else document.documentElement.setAttribute(section.attribute, theme.id);
  };
  sections.forEach(section => setAppearance(section, fromURL(section.parameter)));
  const actions = document.querySelector('.topbar-actions');
  if (!actions || document.getElementById('background-theme-dialog')) return;
  const create = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text) node.textContent = text;
    return node;
  };
  const svg = (pathData, viewBox = '0 0 24 24') => {
    const node = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    node.setAttribute('viewBox', viewBox); node.setAttribute('aria-hidden', 'true');
    const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('d', pathData); node.append(path); return node;
  };
  const trigger = create('button', 'theme-toggle');
  trigger.id = 'background-theme-button'; trigger.type = 'button';
  trigger.setAttribute('aria-haspopup', 'dialog'); trigger.setAttribute('aria-controls', 'background-theme-dialog');
  trigger.append(svg('M12 3a9 9 0 1 0 0 18h1.2a2.2 2.2 0 0 0 1.6-3.7 1.4 1.4 0 0 1 1-2.4h1.7A3.5 3.5 0 0 0 21 11.4 8.6 8.6 0 0 0 12 3ZM7.5 10h.01M10 6.8h.01M14 6.8h.01M17 9.5h.01'));
  trigger.append(create('span', 'theme-toggle-label', 'Izgled'));
  const addSource = actions.querySelector('[data-action="add-source"]');
  if (addSource) actions.insertBefore(trigger, addSource); else actions.append(trigger);
  const dialog = create('dialog', 'theme-dialog'); dialog.id = 'background-theme-dialog';
  dialog.setAttribute('aria-labelledby', 'background-theme-title'); dialog.setAttribute('aria-describedby', 'background-theme-description');
  const heading = create('header', 'theme-dialog-heading');
  const headingText = create('div'); headingText.append(create('p', 'theme-dialog-eyebrow', 'VAŠ PROSTOR'));
  const title = create('h2', '', 'Podesite izgled'); title.id = 'background-theme-title'; headingText.append(title);
  const close = create('button', 'theme-close'); close.type = 'button'; close.setAttribute('aria-label', 'Zatvori podešavanje izgleda'); close.append(svg('M6 6l12 12M6 18 18 6'));
  heading.append(headingText, close);
  const description = create('p', 'theme-dialog-description', 'Kombinujte pozadinu i boje tabela. Svaki izbor se primenjuje odmah.'); description.id = 'background-theme-description';
  const tabs = create('div', 'theme-tabs'); tabs.setAttribute('role', 'tablist'); tabs.setAttribute('aria-label', 'Deo izgleda');
  const panels = {}, tabButtons = {}, buttons = {};
  sections.forEach(section => {
    const tab = create('button', 'theme-tab', section.name); tab.type = 'button'; tab.id = 'appearance-tab-' + section.id;
    tab.dataset.appearanceTab = section.id; tab.setAttribute('role', 'tab'); tab.setAttribute('aria-controls', 'appearance-panel-' + section.id);
    tab.addEventListener('click', () => activateSection(section.id)); tabs.append(tab); tabButtons[section.id] = tab;
    const options = create('div', 'theme-options'); options.id = 'appearance-panel-' + section.id;
    options.dataset.themeSection = section.id; options.setAttribute('role', 'tabpanel'); options.setAttribute('aria-labelledby', tab.id);
    panels[section.id] = options; buttons[section.id] = [];
    themes.forEach(theme => {
      const button = create('button', 'theme-option'); button.type = 'button'; button.dataset.theme = theme.id; button.dataset.themeKind = section.id;
      button.setAttribute('aria-label', theme.name + ' — ' + theme.note); button.setAttribute('aria-pressed', 'false');
      const swatch = create('span', 'theme-swatch'); swatch.setAttribute('aria-hidden', 'true');
      const previewWindow = create('span', 'theme-swatch-window'); const previewContent = create('span', 'theme-swatch-content');
      for (let index = 0; index < 3; index++) previewContent.append(create('span', 'theme-swatch-line'));
      previewWindow.append(create('span', 'theme-swatch-sidebar'), previewContent); swatch.append(previewWindow);
      const check = create('span', 'theme-option-check'); check.setAttribute('aria-hidden', 'true'); check.append(svg('M5 12l4 4L19 6'));
      button.append(swatch, create('span', 'theme-option-label', theme.name), create('span', 'theme-option-note', theme.note), check);
      button.addEventListener('click', () => applyTheme(section, theme, true)); options.append(button); buttons[section.id].push(button);
    });
    options.addEventListener('keydown', event => {
      const choices = buttons[section.id]; const index = choices.indexOf(document.activeElement); if (index < 0) return;
      let next = index;
      if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (index + 1) % choices.length;
      else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = (index + choices.length - 1) % choices.length;
      else if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = choices.length - 1;
      else return;
      event.preventDefault(); choices[next].focus();
    });
  });
  const footer = create('footer', 'theme-dialog-footer');
  const status = create('p', 'theme-selection-status'); status.id = 'appearance-selection-summary'; status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
  const done = create('button', 'theme-done', 'Gotovo'); done.type = 'button'; footer.append(status, done);
  dialog.append(heading, description, tabs, panels.background, panels.table, footer); document.body.append(dialog);
  function updateSummary() {
    const summary = 'Pozadina: ' + current.background.name + ' · Tabele: ' + current.table.name;
    status.textContent = summary; trigger.title = summary; trigger.setAttribute('aria-label', 'Podesite izgled. ' + summary);
  }
  function applyTheme(section, theme, saveURL) {
    setAppearance(section, theme);
    buttons[section.id].forEach(button => button.setAttribute('aria-pressed', String(button.dataset.theme === theme.id)));
    updateSummary();
    if (saveURL) {
      try {
        const url = new URL(window.location.href);
        if (theme.id === 'original') url.searchParams.delete(section.parameter); else url.searchParams.set(section.parameter, theme.id);
        window.history.replaceState(window.history.state, '', url);
      } catch (_) { /* Appearance still changes when URL updates are unavailable. */ }
    }
  }
  function activateSection(id) {
    activeSection = id;
    sections.forEach(section => {
      const active = section.id === id; panels[section.id].hidden = !active;
      tabButtons[section.id].setAttribute('aria-selected', String(active)); tabButtons[section.id].tabIndex = active ? 0 : -1;
    });
  }
  tabs.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 'background' : event.key === 'End' ? 'table' : activeSection === 'background' ? 'table' : 'background';
    activateSection(next); tabButtons[next].focus();
  });
  trigger.addEventListener('click', () => {
    if (dialog.open || typeof dialog.showModal !== 'function') return;
    dialog.showModal();
    const selected = buttons[activeSection].find(button => button.dataset.theme === current[activeSection].id);
    if (selected) selected.focus({ preventScroll: true });
  });
  const closeDialog = () => { if (dialog.open && typeof dialog.close === 'function') dialog.close(); };
  close.addEventListener('click', closeDialog); done.addEventListener('click', closeDialog);
  dialog.addEventListener('close', () => trigger.focus({ preventScroll: true }));
  dialog.addEventListener('click', event => {
    if (event.target !== dialog) return;
    const rect = dialog.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) closeDialog();
  });
  window.addEventListener('popstate', () => sections.forEach(section => applyTheme(section, fromURL(section.parameter), false)));
  sections.forEach(section => applyTheme(section, current[section.id], false)); activateSection(activeSection);
})();
