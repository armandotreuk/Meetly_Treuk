// Replaces console.* calls in src/ with logger.* equivalents and adds the
// logger import. Idempotent: skips files that already import logger.

const fs = require('fs');
const path = require('path');

const SRC = process.argv[2] || 'src';
const root = path.resolve(SRC);

const map = {
    'console.debug': 'logger.debug',
    'console.info': 'logger.info',
    'console.warn': 'logger.warn',
    'console.error': 'logger.error',
    'console.log': 'logger.debug',
};

const needsImportMarker = /\bfrom\s+['"]@\/lib\/logger['"]/;
let total = 0;
let filesTouched = 0;

function processFile(file) {
    let src = fs.readFileSync(file, 'utf8');
    let modified = false;

    for (const [from, to] of Object.entries(map)) {
        const re = new RegExp(from.replace('.', '\\.'), 'g');
        if (re.test(src)) {
            src = src.replace(re, to);
            modified = true;
            total += (src.match(re) || []).length;
        }
    }

    if (!modified) return;

    // Add import if not already present
    if (!needsImportMarker.test(src)) {
        // Find the first line that starts with `import ` and ends with `;`
        const lines = src.split('\n');
        let insertAt = -1;
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i];
            if (/^\s*import\b/.test(line) && line.includes(';')) {
                insertAt = i;
                break;
            }
        }
        const importLine = `import { logger } from '@/lib/logger';`;
        if (insertAt >= 0) {
            lines.splice(insertAt + 1, 0, importLine);
            src = lines.join('\n');
        } else {
            src = `${importLine}\n${src}`;
        }
    }

    fs.writeFileSync(file, src, 'utf8');
    filesTouched += 1;
    console.log(`Updated ${path.relative(process.cwd(), file)}`);
}

function walk(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            if (['node_modules', '.next', 'out', 'dist', 'tests', 'scripts', '__tests__'].includes(entry.name)) continue;
            walk(full);
        } else if (/\.(ts|tsx)$/.test(entry.name)) {
            processFile(full);
        }
    }
}

walk(root);
console.log(`\nDone: ${total} replacements across ${filesTouched} files`);
