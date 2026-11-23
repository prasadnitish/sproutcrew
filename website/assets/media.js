const player = document.querySelector('video');
const start = document.querySelector('.start-film');
const filmButtons = [...document.querySelectorAll('.film-menu button')];
start.hidden = false;
start.addEventListener('click', () => player.play().catch(() => { start.hidden = true; }));
player.addEventListener('play', () => { start.hidden = true; });
for (const button of filmButtons) {
  button.addEventListener('click', () => {
    if (button.getAttribute('aria-pressed') === 'true') return;
    player.pause();
    player.src = button.dataset.src;
    player.poster = button.dataset.poster;
    player.querySelector('track').src = button.dataset.captions;
    player.setAttribute('aria-label', button.textContent);
    document.querySelector('#runtime').textContent = button.dataset.time;
    filmButtons.forEach(item => item.setAttribute('aria-pressed', String(item === button)));
    start.setAttribute('aria-label', 'Play ' + button.textContent);
    start.hidden = false;
    player.load();
  });
}
const dialog = document.querySelector('dialog');
document.querySelectorAll('.shot').forEach(link => link.addEventListener('click', event => {
  if (event.metaKey || event.ctrlKey || event.shiftKey || !dialog.showModal) return;
  event.preventDefault();
  const image = dialog.querySelector('img');
  image.src = link.href;
  image.alt = link.querySelector('img').alt;
  dialog.querySelector('strong').textContent = link.parentElement.querySelector('h3').textContent;
  dialog.showModal();
}));
