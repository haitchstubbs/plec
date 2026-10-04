let rendering;
export function currentRendering() {
    return rendering;
}
export function withRendering(state, callback) {
    const previous = rendering;
    rendering = state;
    try {
        return callback();
    }
    finally {
        rendering = previous;
    }
}
