const motion = matchMedia('(prefers-reduced-motion: reduce)');
const deck = document.querySelector('.deck');
if (deck) {
  const run = deck.closest('.tour-run');
  const stage = run.querySelector('.tour-stage');
  const track = deck.querySelector('.track');
  const slides = [...track.children];
  const status = deck.querySelector('.step-status');
  const [previous, next] = deck.querySelectorAll('.arrows button');
  const header = document.querySelector('.mast');
  const milestones = [...deck.querySelectorAll('[data-milestone]')];
  let distance = 0;
  let inset = 0;
  let active = -1;
  let frame = 0;

  function show(index) {
    if (index === active) return;
    active = index;
    status.textContent = (index + 1) + ' / ' + slides.length + ' · ' + slides[index].dataset.title;
    previous.disabled = index === 0;
    next.disabled = index === slides.length - 1;
    slides.forEach((slide, i) => {
      slide.inert = i !== index;
      slide.classList.toggle('entered', i === index);
    });
    milestones.forEach((link, i) => {
      if (i === index) link.setAttribute('aria-current', 'step');
      else link.removeAttribute('aria-current');
    });
    if (index > 0) deck.querySelector('video')?.pause();
  }

  function render() {
    frame = 0;
    if (distance) {
      const progress = Math.min(slides.length - 1, Math.max(0, (inset - run.getBoundingClientRect().top) / distance));
      track.scrollLeft = progress * track.clientWidth;
    }
    show(Math.round(track.scrollLeft / track.clientWidth));
  }

  function schedule() {
    if (!frame) frame = requestAnimationFrame(render);
  }

  function layout() {
    inset = Math.min(header.offsetHeight + 6, window.innerHeight - stage.offsetHeight);
    distance = motion.matches ? 0 : Math.round(Math.min(800, Math.max(320, innerHeight * .68)));
    run.style.setProperty('--travel', distance * (slides.length - 1) + 'px');
    run.style.setProperty('--pin-top', inset + 'px');
    run.classList.toggle('pinned', Boolean(distance));
    render();
  }

  function move(index, animate = true) {
    const target = Math.max(0, Math.min(index, slides.length - 1));
    const behavior = motion.matches || !animate ? 'instant' : 'smooth';
    if (distance) {
      const start = scrollY + run.getBoundingClientRect().top - inset;
      window.scrollTo({top: start + target * distance, behavior});
    } else {
      track.scrollTo({left: target * track.clientWidth, behavior});
    }
  }

  milestones.forEach((link, i) => link.addEventListener('click', event => {
    event.preventDefault();
    move(i);
  }));
  function followRoadmapLink() {
    if (!milestones.length || !location.hash) return;
    const target = document.getElementById(location.hash.slice(1));
    const index = slides.indexOf(target);
    if (index >= 0) move(index, false);
    if (target?.matches('.road-notes details')) {
      target.open = true;
      target.scrollIntoView();
    }
  }
  document.querySelectorAll('.phase-copy a').forEach(link => link.addEventListener('click', () => {
    document.getElementById(link.hash.slice(1)).open = true;
  }));
  window.addEventListener('hashchange', followRoadmapLink);
  previous.onclick = () => move(active - 1);
  next.onclick = () => move(active + 1);
  track.addEventListener('keydown', event => {
    if (event.target !== track || !['ArrowLeft','ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    move(active + (event.key === 'ArrowLeft' ? -1 : 1));
  });
  let touchStart;
  track.addEventListener('pointerdown', event => {
    if (distance && event.pointerType === 'touch') touchStart = [event.clientX, event.clientY];
  });
  track.addEventListener('pointercancel', () => {touchStart = null;});
  track.addEventListener('pointerup', event => {
    if (!touchStart) return;
    const dx = event.clientX - touchStart[0], dy = event.clientY - touchStart[1];
    touchStart = null;
    if (Math.abs(dx) > 48 && Math.abs(dx) > Math.abs(dy)) move(active + (dx < 0 ? 1 : -1));
  });
  window.addEventListener('scroll', schedule, {passive:true});
  track.addEventListener('scroll', () => { if (!distance) schedule(); }, {passive:true});
  window.addEventListener('resize', layout);
  motion.addEventListener('change', layout);
  const observer = new ResizeObserver(layout);
  observer.observe(stage);
  observer.observe(header);
  layout();
  window.addEventListener('pageshow', followRoadmapLink);
}
const topic = document.querySelector('#topic');
if (topic) {
  const sections = [...document.querySelectorAll('.answers')];
  const field = document.querySelector('#question');
  const response = document.querySelector('#response');
  function browse(id) {
    const section = sections.find(section => section.id === id);
    if (!section) return;
    topic.value = id;
    sections.forEach(other => {other.hidden = other !== section;});
  }
  function followSource() {
    const target = document.getElementById(location.hash.slice(1));
    const section = target?.closest('.answers');
    if (!section) {browse(topic.value);return;}
    browse(section.id);
    if (target.tagName === 'DETAILS') target.open = true;
    target.scrollIntoView({block:'start'});
  }
  function stepTopic(direction) {
    const i = (topic.selectedIndex + direction + sections.length) % sections.length;
    browse(sections[i].id);
  }
  document.querySelector('#prev-topic').onclick = () => stepTopic(-1);
  document.querySelector('#next-topic').onclick = () => stepTopic(1);
  response.addEventListener('click', event => {
    const link = event.target.closest('a[href^="#"]');
    if (link && link.hash === location.hash) followSource();
  });
  topic.addEventListener('change',() => browse(topic.value));
  window.addEventListener('hashchange',followSource);
  followSource();
  const stop = new Set('a an the and are be by do does for from how i in is it me my of on or sprout sproutcrew that to what will with you your can much about there'.split(' '));
  function words(text) {
    const aliases = {price:'cost',pricing:'cost',costs:'cost',budget:'spend',spending:'spend',limits:'limit',password:'key',keys:'key',stored:'kept',export:'files',formats:'files',outputs:'files',output:'files',charge:'cost',charged:'cost'};
    return [...new Set((text.toLowerCase().match(/[a-z0-9]+/g)||[]).filter(w=>!stop.has(w)).map(w=>aliases[w]||w))];
  }
  const index = helpEntries.map(entry=>({...entry,title:words(entry.q),body:words(entry.a)}));
  const aliases = new Map([
    ['how much can a project spend', 'Is there a spending cap?'],
    ['what is the project budget', 'Is there a spending cap?'],
    ['where are my api keys stored', 'Where are my keys kept?'],
    ['what formats can i export', 'What files do I get?']
  ]);
  function answer(query) {
    response.replaceChildren();
    query = aliases.get(query.trim().toLowerCase().replace(/[?.!]+$/, '')) || query;
    const tokens = words(query);
    const exact = index.find(entry=>entry.q.toLowerCase()===query.trim().toLowerCase());
    const ranked = index.map(entry=>{
      const hits=tokens.filter(t=>entry.title.includes(t));
      const total=tokens.filter(t=>entry.title.includes(t)||entry.body.includes(t)).length;
      return {entry,score:hits.length*5+total,qualifies:hits.length>0 && total>=Math.ceil(tokens.length*.65)};
    }).filter(match=>match.qualifies).sort((a,b)=>b.score-a.score);
    const match = exact || (ranked.length && (ranked.length===1 || ranked[0].score>ranked[1].score) ? ranked[0].entry : null);
    if (!match) {
      const notice=document.createElement('p');
      notice.textContent=ranked.length?'Which guide question fits?':'I couldn’t find a supported answer. Try a product topic below.';
      response.append(notice);
      ranked.slice(0,3).forEach(({entry})=>{const a=document.createElement('a');a.href='#'+entry.id;a.textContent=entry.q;response.append(a);});
      return;
    }
    const card=document.createElement('article'),title=document.createElement('h2'),p=document.createElement('p'),source=document.createElement('a');
    title.textContent=match.q;p.textContent=match.a;source.href='#'+match.id;source.textContent='Read guide entry →';
    card.append(title,p,source);
    const revision = 'https://github.com/prasadnitish/bff/blob/8084b5667ac5daf6422039e61b5ff525bc6a965e/';
    const reference = document.createElement('a');
    reference.href = revision + 'website/help.html';
    reference.textContent = 'Guide source ↗';
    card.append(reference);
    const codeRefs = {
      'Is there a spending cap?': ['spend.rs#L12', 'Default cap'],
      'Where are my keys kept?': ['keychain.rs#L119', 'Keychain storage']
    };
    const evidence = codeRefs[match.q];
    if (evidence) {
      const link = document.createElement('a');
      link.href = revision + 'crates/bff-core/src/' + evidence[0];
      link.textContent = evidence[1] + ' in code ↗';
      card.append(link);
    }
    response.append(card);
  }
  document.querySelector('#ask').addEventListener('submit',event=>{event.preventDefault();answer(field.value);});
  document.querySelectorAll('.suggestions button').forEach(button=>button.addEventListener('click',()=>{field.value=button.textContent;answer(field.value);}));
}
