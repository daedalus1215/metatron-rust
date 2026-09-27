import { JSDOM, VirtualConsole } from 'jsdom';
import { readFileSync, readdirSync } from 'fs';

const dir = process.argv[2];
let bad = 0;
for (const f of readdirSync(dir).filter(f => f.endsWith('.html')).sort()) {
  const errs = [];
  let draws = 0;
  const vc = new VirtualConsole();
  vc.on('jsdomError', e => errs.push(e.message.split('\n')[0]));
  vc.on('error', (...a) => errs.push('console.error: ' + a.join(' ')));
  let dom;
  try {
    dom = new JSDOM(readFileSync(`${dir}/${f}`, 'utf8'), {
      runScripts: 'dangerously', pretendToBeVisual: true, virtualConsole: vc,
      // Count real drawing before anything runs. A payload shaped as an
      // object where the template indexed an array throws nothing at all:
      // it draws nothing, and every other check calls that a pass.
      beforeParse(w) {
        // node-canvas returns its own context object, not
        // window.CanvasRenderingContext2D, so wrap at the source.
        const gc = w.HTMLCanvasElement.prototype.getContext;
        w.HTMLCanvasElement.prototype.getContext = function (...a) {
          const ctx = gc.apply(this, a);
          if (!ctx || ctx.__counted) return ctx;
          ctx.__counted = true;
          for (const m of ['stroke', 'fill', 'fillText', 'fillRect']) {
            const o = ctx[m];
            if (typeof o === 'function') ctx[m] = (...b) => { draws++; return o.apply(ctx, b); };
          }
          return ctx;
        };
      },
    });
    await new Promise(r => setTimeout(r, 400));
  } catch (e) { errs.push('load: ' + e.message); }
  const doc = dom?.window?.document;
  const nodes = doc?.querySelectorAll('*')?.length ?? 0;
  const svgs = doc?.querySelectorAll('svg *')?.length ?? 0;
  const hasCanvas = !!doc?.querySelector('canvas');
  // A view is blank if its own medium drew nothing.
  const thin = nodes < 30 || (hasCanvas ? draws < 100 : (svgs < 5 && nodes < 200 && f !== 'index.html'));
  const status = errs.length ? 'ERROR' : (thin ? 'BLANK' : 'ok');
  if (errs.length || thin) bad++;
  console.log(`${f.padEnd(15)} ${status.padEnd(6)} ${String(nodes).padStart(5)} nodes ${String(svgs).padStart(5)} svg ${String(draws).padStart(6)} draws`);
  errs.slice(0, 2).forEach(e => console.log('   ! ' + e.slice(0, 140)));
}
process.exit(bad ? 1 : 0);
