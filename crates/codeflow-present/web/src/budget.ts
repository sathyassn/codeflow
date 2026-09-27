// Fitting a review's crops into the service's body limit (QA defect 3). The
// limit stays (SPC-014); the page shares what the notes leave of it among the
// crops, smallest first. A crop over its share is re-encoded at a lower JPEG
// quality, then smaller, down to a floor; one that still does not fit is left
// out. No note is ever dropped: it keeps its body, selector and quote.

type Crop = { readonly media_type: "image/jpeg" | "image/png"; readonly data_base64: string };
type WithExcerpt = { readonly excerpt?: { readonly text?: string; readonly image?: Crop } };

const EMPTY: Crop = { media_type: "image/jpeg", data_base64: "" };
const QUALITIES = [0.7, 0.55, 0.4, 0.3];
const SHRINK = 0.75;
/** The smallest a crop is made: its longer side in pixels. */
export const CROP_FLOOR_PX = 64;

export interface FitResult<N> {
  readonly notes: readonly N[];
  readonly reduced: number;
  readonly dropped: number;
}

/** Re-encodes one crop to at most `limit` base64 characters, or null. */
export type Reencode = (image: Crop, limit: number) => Promise<Crop | null>;

/**
 * Fits the crops of `notes` so that `size(notes)` (the encoded request size in
 * bytes) is at most `cap`. Returns the notes unchanged when they already fit.
 */
export async function fitCrops<N extends WithExcerpt>(
  notes: readonly N[],
  cap: number,
  size: (notes: readonly N[]) => number,
  reencode: Reencode = reencodeJpeg,
): Promise<FitResult<N>> {
  if (size(notes) <= cap) return { notes, reduced: 0, dropped: 0 };
  // Every crop's JSON wrapper is charged up front (as a JPEG, the longer
  // type name); base64 carries no escapes, so each crop then costs its own
  // length. A crop left out gives its wrapper back.
  const wrapped = notes.map((note) => withImage(note, EMPTY));
  let room = cap - size(wrapped);
  const order = notes
    .map((note, index) => ({ index, length: note.excerpt?.image?.data_base64.length ?? 0 }))
    .filter((item) => item.length > 0)
    .sort((left, right) => left.length - right.length || left.index - right.index);
  const images = new Map<number, Crop | undefined>();
  let reduced = 0;
  let dropped = 0;
  for (const [position, { index, length }] of order.entries()) {
    const share = Math.floor(Math.max(0, room) / (order.length - position));
    const image = notes[index]!.excerpt!.image!;
    let fitted: Crop | null = length <= share ? image : null;
    if (!fitted && share > 0) {
      fitted = await reencode(image, share);
      if (fitted) reduced += 1;
    }
    if (!fitted) dropped += 1;
    images.set(index, fitted ?? undefined);
    room -= fitted?.data_base64.length ?? 0;
  }
  const fittedNotes = notes.map((note, index) => (images.has(index) ? withImage(note, images.get(index)) : note));
  // Anything left over (bodies alone over the cap) is the caller's refusal.
  return { notes: fittedNotes, reduced, dropped };
}

function withImage<N extends WithExcerpt>(note: N, image: Crop | undefined): N {
  if (!note.excerpt?.image) return note;
  const { image: _old, ...rest } = note.excerpt;
  const excerpt = image ? { ...rest, image } : rest;
  const next = { ...note } as { excerpt?: unknown };
  if (Object.keys(excerpt).length) next.excerpt = excerpt;
  else delete next.excerpt;
  return next as N;
}

/** The browser re-encoder: lower JPEG quality first, then smaller, to the floor. */
export async function reencodeJpeg(image: Crop, limit: number): Promise<Crop | null> {
  const source = new Image();
  source.src = `data:${image.media_type};base64,${image.data_base64}`;
  try {
    await source.decode();
  } catch {
    return null;
  }
  let width = source.naturalWidth;
  let height = source.naturalHeight;
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context || width < 1 || height < 1) return null;
  for (;;) {
    canvas.width = Math.max(1, Math.round(width));
    canvas.height = Math.max(1, Math.round(height));
    // JPEG has no alpha; a transparent PNG crop is laid on white.
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.drawImage(source, 0, 0, canvas.width, canvas.height);
    for (const quality of QUALITIES) {
      const data = canvas.toDataURL("image/jpeg", quality).split(",", 2)[1] ?? "";
      if (data && data.length <= limit) return { media_type: "image/jpeg", data_base64: data };
    }
    if (Math.max(width, height) * SHRINK < CROP_FLOOR_PX) return null;
    width *= SHRINK;
    height *= SHRINK;
  }
}
