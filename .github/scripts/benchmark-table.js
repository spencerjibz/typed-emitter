const fs = require('fs');
const path = require('path');

const MARKER = '<!-- benchmark-comparison -->';
const ID_PATTERN = /^[A-Za-z0-9_+./-]{1,120}$/;
const MAX_RESULTS = 200;

function findBenchmarks(dir, baseline) {
     const found = [];
     for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
          if (!entry.isDirectory()) continue;
          const child = path.join(dir, entry.name);
          if (fs.existsSync(path.join(child, baseline, 'estimates.json'))) found.push(child);
          else found.push(...findBenchmarks(child, baseline));
     }
     return found;
}

function estimate(dir, baseline) {
     const file = path.join(dir, baseline, 'estimates.json');
     if (!fs.existsSync(file)) return null;
     return JSON.parse(fs.readFileSync(file, 'utf8')).mean.point_estimate;
}

function collect(criterionDir, base, pr, baseUsesPrBenches) {
     const dirs = fs.existsSync(criterionDir) ? findBenchmarks(criterionDir, pr) : [];
     const rows = dirs
          .map((dir) => ({
               id: JSON.parse(fs.readFileSync(path.join(dir, pr, 'benchmark.json'), 'utf8')).full_id,
               base: estimate(dir, base),
               pr: estimate(dir, pr),
          }))
          .sort((a, b) => a.id.localeCompare(b.id));
     const results = rows.filter((row) => row.base !== null);
     const comparisons = rows
          .filter((row) => row.base === null && row.id.startsWith('compare_'))
          .map(({ id, pr: value }) => ({ id, pr: value }));
     return comparisons.length > 0
          ? { baseUsesPrBenches, results, comparisons }
          : { baseUsesPrBenches, results };
}

function hasExactKeys(value, keys) {
     return (
          typeof value === 'object' &&
          value !== null &&
          !Array.isArray(value) &&
          Object.keys(value).sort().join() === keys
     );
}

function validate(data) {
     const shape = ['baseUsesPrBenches,results', 'baseUsesPrBenches,comparisons,results'];
     if (
          !shape.some((keys) => hasExactKeys(data, keys)) ||
          typeof data.baseUsesPrBenches !== 'boolean'
     ) {
          throw new Error('invalid benchmark results');
     }
     const comparisons = 'comparisons' in data ? data.comparisons : [];
     for (const [rows, keys, values] of [
          [data.results, 'base,id,pr', (row) => [row.base, row.pr]],
          [comparisons, 'id,pr', (row) => [row.pr]],
     ]) {
          if (!Array.isArray(rows) || rows.length > MAX_RESULTS) {
               throw new Error('invalid benchmark results');
          }
          for (const row of rows) {
               const valid =
                    hasExactKeys(row, keys) &&
                    typeof row.id === 'string' &&
                    ID_PATTERN.test(row.id) &&
                    values(row).every((v) => typeof v === 'number' && Number.isFinite(v) && v > 0);
               if (!valid) throw new Error('invalid benchmark result');
          }
     }
     return { ...data, comparisons };
}

function formatTime(ns) {
     for (const [unit, scale] of [['s', 1e9], ['ms', 1e6], ['µs', 1e3]]) {
          if (ns >= scale) return `${(ns / scale).toFixed(2)} ${unit}`;
     }
     return `${ns.toFixed(2)} ns`;
}

function render(data, baseRef) {
     const ref = String(baseRef).replace(/[^A-Za-z0-9._/-]/g, '');
     const lines = [MARKER, `### Benchmarks against \`${ref}\``, ''];
     if (data.results.length === 0) {
          lines.push(`Benchmarks could not run on \`${ref}\`, so there is nothing to compare against.`);
     } else {
          if (data.baseUsesPrBenches) {
               lines.push(`\`${ref}\` has no benchmarks, so it was measured with this PR's.`, '');
          }
          lines.push(`| Benchmark | \`${ref}\` | this PR | Change |`, '|---|---:|---:|---:|');
          for (const { id, base, pr } of data.results) {
               const change = ((pr - base) / base) * 100;
               const sign = change > 0 ? '+' : '';
               lines.push(`| \`${id}\` | ${formatTime(base)} | ${formatTime(pr)} | ${sign}${change.toFixed(1)}% |`);
          }
     }
     if (data.comparisons.length > 0) {
          const groups = new Map();
          for (const { id, pr } of data.comparisons) {
               const split = id.lastIndexOf('/');
               const group = split > 0 ? id.slice(0, split) : id;
               const name = split > 0 ? id.slice(split + 1) : id;
               if (!groups.has(group)) groups.set(group, new Map());
               groups.get(group).set(name, pr);
          }
          const names = [...new Set([...groups.values()].flatMap((g) => [...g.keys()]))].sort();
          lines.push('', '#### Compared with other emitters (this PR)', '');
          lines.push(`| Benchmark | ${names.map((n) => `\`${n}\``).join(' | ')} |`);
          lines.push(`|---|${names.map(() => '---:').join('|')}|`);
          for (const [group, values] of groups) {
               const cells = names.map((n) => (values.has(n) ? formatTime(values.get(n)) : '-'));
               lines.push(`| \`${group}\` | ${cells.join(' | ')} |`);
          }
     }
     return `${lines.join('\n')}\n`;
}

module.exports = { collect, validate, render };

if (require.main === module) {
     const [command, ...args] = process.argv.slice(2);
     if (command === 'collect') {
          const [criterionDir, base, pr] = args;
          const data = collect(criterionDir, base, pr, process.env.BASE_USES_PR_BENCHES === 'true');
          process.stdout.write(`${JSON.stringify(data, null, 2)}\n`);
     } else if (command === 'render') {
          const [file, baseRef] = args;
          process.stdout.write(render(validate(JSON.parse(fs.readFileSync(file, 'utf8'))), baseRef));
     } else {
          console.error('usage: benchmark-table.js collect <criterion-dir> <base> <pr> | render <results.json> <base-ref>');
          process.exit(1);
     }
}
