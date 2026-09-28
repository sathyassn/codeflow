// How much a note's crop looks like the page it anchors (TSK-071, G071-1).
// The runtime paints its own crop, so pictures are compared by where the
// words, lines and shapes fall, not pixel for pixel. The functions are
// self-contained: the annotation matrix runs them inside the page on decoded
// images, and likeness.test.mjs runs them in Node on synthetic pictures.

// The lowest inside likeness a crop may have. The 30 true crops of the
// annotation matrix score 0.90 to 1.00 (the tabs and diff elements lowest;
// the tabs crop lacks the summary's ::marker, which the runtime cannot
// paint). In likeness.test.mjs a stripe field scores up to 0.20, a crop of
// the same rectangle one or two lines off up to 0.42, another text 0.12, the
// right ink moved 4 or 12 px up to 0.82, and a blank crop 0. The floor sits
// between them, and the registration check below catches small shifts too.
export const LIKENESS_FLOOR = 0.85;

// A crop in place is at least as like its own rectangle as the same rectangle
// moved 4 or 12 px any way (text repeats along a line, so one distance is
// not enough), less this slack for noise on smooth pictures that barely
// change over a few pixels.
export const NEARBY_OFFSETS = [4, 12].flatMap((step) => [[0, -step], [0, step], [-step, 0], [step, 0]]);
export const REGISTRATION_SLACK = 0.03;

/**
 * The grid a picture of this size is compared on: 48 columns, and a row for
 * every 3 px of height (4 to 32), so a line of text spans several rows.
 */
export function gridShape(width, height) {
  const columns = 48;
  return { columns, rows: Math.min(32, Math.max(4, Math.round(height / 3))) };
}

/**
 * Where the ink is: each pixel that stands apart from the picture's own ground
 * (its median brightness), as a fraction of each grid cell. `pixels` is RGBA.
 */
export function inkGrid(pixels, width, height, columns, rows) {
  const light = new Float32Array(width * height);
  for (let index = 0; index < light.length; index += 1) {
    light[index] = 0.2126 * pixels[index * 4] + 0.7152 * pixels[index * 4 + 1] + 0.0722 * pixels[index * 4 + 2];
  }
  const ground = [...light].sort((left, right) => left - right)[light.length >> 1];
  const ink = new Float32Array(columns * rows);
  const count = new Float32Array(columns * rows);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const cell = Math.min(rows - 1, Math.floor(y * rows / height)) * columns + Math.min(columns - 1, Math.floor(x * columns / width));
      count[cell] += 1;
      if (Math.abs(light[y * width + x] - ground) > 48) ink[cell] += 1;
    }
  }
  return [...ink].map((value, cell) => value / Math.max(1, count[cell]));
}

/**
 * The correlation of two ink grids, each row taken less its own mean: 1 for
 * the same layout of ink, near 0 for unrelated ink, and 0 for a picture with
 * no ink structure at all. Text lines share their rhythm of ink and gap, so
 * only where the words fall along each row tells one line from the next.
 * `columns` is the grid's width (gridShape's 48).
 */
export function likeness(left, right, columns = 48) {
  const alongRows = (grid) => grid.map((value, cell) => {
    const start = cell - (cell % columns);
    let sum = 0;
    for (let index = start; index < start + columns; index += 1) sum += grid[index];
    return value - sum / columns;
  });
  [left, right] = [alongRows(left), alongRows(right)];
  const mean = (values) => values.reduce((sum, value) => sum + value, 0) / values.length;
  const [a, b] = [mean(left), mean(right)];
  let product = 0;
  let first = 0;
  let second = 0;
  for (const [index, value] of left.entries()) {
    product += (value - a) * (right[index] - b);
    first += (value - a) ** 2;
    second += (right[index] - b) ** 2;
  }
  return first > 1e-9 && second > 1e-9 ? product / Math.sqrt(first * second) : 0;
}

/**
 * A crop is in place when it clears the floor, beats the page just outside
 * it, and is in register with its rectangle.
 */
export function placed(inside, outside, nearby = [], floor = LIKENESS_FLOOR) {
  return inside >= floor && inside > outside && nearby.every((value) => inside >= value - REGISTRATION_SLACK);
}
