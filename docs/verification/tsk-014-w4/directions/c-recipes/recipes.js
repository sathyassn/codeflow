/* DIRECTION C — project-neutral recipe library.
 *
 * The mechanism this direction is demonstrated by. The runtime executes the
 * same curated primitives as direction A. What moves is where composition
 * authority sits: a *library* of named, parameterised recipes ships as a
 * managed skill asset, versioned with the skill rather than with the binary. A
 * document names a recipe and binds real data to its declared slots.
 *
 * The consequence, which is the whole point of separating this from A: adding a
 * new subject form is a reviewed managed asset — no binary release, no schema
 * version, no renderer change — but it is still a review, not a document. An
 * author who needs a form the library has no recipe for is refused, and the
 * refusal names the contribution path.
 */
(() => {
  class NoRecipe extends Error {}
  class UnboundSlot extends Error {}

  /* Each recipe declares its slots and compiles a binding into a primitive
   * block. It contains no geometry: the primitives own that, as in direction A. */
  const LIBRARY = {
    'schedule-room': {
      version: '1.0.0',
      about: 'Objects on an ordered planning axis, each with the room it has before it moves the end.',
      slots: ['items', 'axis', 'run', 'vocabulary'],
      compile: (bind, id) => ({
        primitive: 'extent_on_axis',
        id,
        title: bind.vocabulary.title,
        description: bind.vocabulary.description,
        unit: bind.vocabulary.unit,
        axis: bind.axis,
        runLabel: bind.vocabulary.runLabel,
        extentLabel: bind.vocabulary.extentLabel,
        capLabel: bind.vocabulary.capLabel,
        rows: bind.items,
        run: bind.run,
      }),
    },
    'derivation-stop': {
      version: '1.2.0',
      about: 'Objects that travel through ordered derivation stages and stop, with the cause of each stop.',
      slots: ['items', 'stages', 'vocabulary'],
      compile: (bind, id) => ({
        primitive: 'travel_and_stop',
        id,
        title: bind.vocabulary.title,
        description: bind.vocabulary.description,
        unit: bind.vocabulary.unit,
        stages: bind.stages,
        rows: bind.items,
      }),
    },
    'lane-coverage': {
      version: '1.0.1',
      about: 'Objects against a fixed set of lanes, each cell in a closed state vocabulary.',
      slots: ['items', 'lanes', 'vocabulary'],
      compile: (bind, id) => ({
        primitive: 'coverage_grid',
        id,
        title: bind.vocabulary.title,
        description: bind.vocabulary.description,
        unit: bind.vocabulary.unit,
        lanes: bind.lanes,
        rows: bind.items,
      }),
    },
  };

  function resolve(block) {
    const recipe = LIBRARY[block.recipe];
    if (!recipe) {
      throw new NoRecipe(
        `the library has no recipe "${block.recipe}". It carries ${Object.keys(LIBRARY).map((k) => `${k}@${LIBRARY[k].version}`).join(', ')}. ` +
        'Adding one is a reviewed managed asset — a recipe document versioned with the skill — not a binary release, and not something this document may do inline.',
      );
    }
    for (const slot of recipe.slots) {
      if (!(slot in block.bind)) throw new UnboundSlot(`recipe "${block.recipe}" declares slot "${slot}" and the document did not bind it`);
    }
    return recipe.compile(block.bind, block.id);
  }

  function mount(host, block, breakpoints) {
    const paint = () => {
      const w = window.innerWidth;
      const mode = w <= breakpoints.narrow ? 'narrow' : w <= breakpoints.wide ? 'intermediate' : 'wide';
      const width = mode === 'narrow' ? 344 : mode === 'intermediate' ? 840 : 1150;
      try {
        const compiled = resolve(block);
        const svg = window.w4primitives.render(compiled, { width, mode });
        svg.dataset.emittedBy = `c-recipes:${block.recipe}@${LIBRARY[block.recipe].version} -> ${svg.dataset.emittedBy}`;
        host.replaceChildren(svg);
        host.dataset.band = mode;
      } catch (error) {
        host.replaceChildren(
          Object.assign(document.createElement('p'), { className: 'sh-note', textContent: `The runtime refused this block: ${error.message}` }),
        );
        host.dataset.band = mode;
        host.dataset.refused = 'true';
      }
    };
    paint();
    window.addEventListener('resize', paint);
  }

  window.w4recipes = { LIBRARY, resolve, mount, NoRecipe, UnboundSlot };
})();
