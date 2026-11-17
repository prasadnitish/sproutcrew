// App-owned layout code. Input is a bounded Rust Markdown document; no model-authored code runs.
async function renderDeck(input) {
  const pptx = new PptxGenJS();
  pptx.layout = 'LAYOUT_WIDE';
  pptx.author = 'SproutCrew'; pptx.subject = input.subtitle; pptx.title = input.title;
  pptx.company = 'SproutCrew'; pptx.lang = 'en-US';
  // The look comes from the app as data: a checked font name and six hex digits. SproutCrew's own is the default.
  const LOOK = input.look || {};
  const FONT = LOOK.font || 'Helvetica Neue';
  pptx.theme = { headFontFace: LOOK.heading_font || FONT, bodyFontFace: FONT, lang: 'en-US' };
  const BLUE = LOOK.accent || '3347E0', CREAM = 'F7F3EC', INK = '14161C';
  const HEAD = LOOK.heading_font || FONT;
  const master = [{
    placeholder:{options:{name:'slideTitle',type:'title',x:0.65,y:0.4,w:12,h:0.8,fontFace:HEAD,fontSize:27,bold:true,color:BLUE,margin:0,fit:'shrink'}}
  }];
  // A logo from the template, about 0.4 inch high in the top right corner of every slide.
  if (LOOK.logo && LOOK.logo_px && LOOK.logo_px[1] > 0) {
    const h = 0.4, w = Math.min(2.5, h * LOOK.logo_px[0] / LOOK.logo_px[1]);
    master.push({ image: { data: 'image/png;base64,' + LOOK.logo, x: 12.98 - w, y: 0.15, w, h } });
  }
  pptx.defineSlideMaster({title:'SPROUT',background:{color:CREAM},objects:master});
  let slide, y, heading = input.title, part = 1;
  function footer() {
    if (slide) slide.addText(String(pptx._slides.length), { x: 12, y: 7.1, w: 0.65, h: 0.2, fontSize: 10, color: '666666', align: 'right', margin: 0, objectName:'Page number' });
  }
  function fresh(title, continued = false) {
    if (pptx._slides.length >= 250) throw new Error('Deck exceeds 250 slides');
    footer(); part = continued ? part+1 : 1;
    slide = pptx.addSlide({masterName:'SPROUT'});
    slide.addText(title + (continued ? ` (continued ${part})` : ''), { placeholder:'slideTitle', objectName:'Slide title' });
    y = 1.5;
  }
  function rich(runs) {
    return runs.map(r => ({ text: r.text, options: { bold: r.bold, italic: r.italic, strike: r.strike, fontFace: r.code ? 'Menlo' : FONT, ...(r.link ? {hyperlink: {url: r.link}, color: BLUE, underline: true} : {}) } }));
  }
  // Conservative word wrapping preserves all characters. Each fragment has fixed bounded geometry.
  function chunks(runs, max = 82) {
    const result = []; let line = [], count = 0;
    const width = c => /[\u2e80-\uffff]/u.test(c) || c.codePointAt(0) > 0xffff ? 2 : /[MW@]/u.test(c) ? 1.7 : /\s/u.test(c) ? 0.5 : 1;
    for (const run of runs) {
      for (const token of run.text.split(/(\n|\s+)/u).filter(Boolean)) {
        if (token === '\n') { result.push(line); line = []; count = 0; continue; }
        const chars = Array.from(token), totalWidth = chars.reduce((n,c)=>n+width(c),0);
        if (count && count + totalWidth > max) { result.push(line); line = []; count = 0; }
        let part = '';
        for (const c of chars) {
          if (count + width(c) > max) {
            if (part) line.push({...run,text:part});
            result.push(line); line = []; count = 0; part = '';
          }
          part += c; count += width(c);
        }
        if (part) line.push({...run,text:part});
      }
    }
    if (line.length) result.push(line);
    return result;
  }
  fresh(input.title);
  slide.addText(input.subtitle, {x:0.7,y:2.25,w:11.8,h:1.2,fontSize:21,color:INK,margin:0,fit:'shrink'});
  let started = true; y = 3.6;
  for (const block of input.blocks) {
    if (block.kind === 'notes') { slide.addNotes(block.runs.map(r=>r.text).join('')); continue; }
    if (block.kind === 'heading' && block.level <= 2) {
      heading = block.runs.map(r=>r.text).join(''); fresh(heading); started = true; continue;
    }
    if (!started) { fresh(heading); started = true; }
    if (block.kind === 'rule') { if (y > 6.5) fresh(heading,true); y += 0.2; continue; }
    if (block.kind === 'table') {
      // Editable tables, at most six columns per group. Long rows continue with repeated headers.
      const count = Math.max(...block.rows.map(row=>row.length));
      for (let col = 0; col < count; col += 6) {
        const columns = Math.min(6,count-col), colWidth = 11.7/columns;
        const maxUnits = Math.max(8,Math.floor(colWidth*72/8.5)-2);
        const linesFor = row => Array.from({length:columns}, (_,i)=>chunks(row[col+i] || [],maxUnits));
        const headers = linesFor(block.rows[0]);
        const headerLines = Math.max(1,...headers.map(cell=>cell.length));
        if (headerLines > 8) throw new Error('Table headers are too long; shorten them before exporting');
        const asCell = (lines,bold=false) => ({text:rich(lines.flatMap((line,i)=>i ? [{text:'\n'},...line] : line)),options:{bold,fill:bold?'E7EBFF':'FFFFFF',color:INK,margin:0.06}});
        const bodyRows = block.rows.length > 1 ? block.rows.slice(1) : [[]];
        for (const row of bodyRows) {
          const cells = linesFor(row);
          const lineCount = Math.max(1,...cells.map(cell=>cell.length));
          for (let at = 0; at < lineCount; at += 7) {
            const take = Math.min(7,lineCount-at);
            const height = (headerLines+take)*0.3+0.3;
            if (y + height > 6.75) fresh(heading,true);
            slide.addTable([headers.map(cell=>asCell(cell,true)),cells.map(cell=>asCell(cell.slice(at,at+take)))],{
              x:0.75,y,w:11.7,h:height,colW:Array(columns).fill(colWidth),fontFace:FONT,fontSize:14,
              border:{type:'solid',color:'C8CED8',pt:0.6},autoPage:false,margin:0.06,breakLine:false,
            });
            y += height+0.15;
          }
        }
        if (col+6 < count) fresh(heading,true);
      }
      continue;
    }
    const prefix = block.marker ? [{text: block.marker+'  '}] : [];
    const lines = chunks([...prefix,...block.runs], block.kind === 'code' ? 85 : 72);
    const size = block.kind === 'code' ? 14 : block.kind === 'heading' ? 21 : 19;
    const lineHeight = size === 21 ? 0.42 : size === 14 ? 0.30 : 0.36;
    for (const line of lines) {
      if (y + lineHeight > 6.8) fresh(heading,true);
      const indent = Math.min(block.indent, 6) * 0.17;
      slide.addText(rich(line),{x:0.7+indent,y,w:11.8-indent,h:lineHeight,fontSize:size,bold:block.kind==='heading',color:INK,margin:0,breakLine:false});
      y += lineHeight;
    }
    y += 0.13;
  }
  footer();
  // PptxGenJS emits tblPr without header semantics. Mark our explicit first rows as headers.
  // Only app-generated XML is processed; no source XML, templates or macros are accepted.
  const zip = await JSZip.loadAsync(await pptx.write({outputType:'uint8array', compression:true}));
  for (const name of Object.keys(zip.files).filter(n=>/^ppt\/slides\/slide\d+\.xml$/u.test(n))) {
    const xml = await zip.file(name).async('string');
    if (xml.includes('<a:tbl>')) zip.file(name,xml.replaceAll('<a:tblPr/>','<a:tblPr firstRow="1"/>'));
  }
  return zip.generateAsync({type:'base64',compression:'DEFLATE'});
}
