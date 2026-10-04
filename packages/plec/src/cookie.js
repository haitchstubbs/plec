/** Declarative cookie capability. Calls are compiled; direct execution is not supported. */
export const cookie = {
    getSync(_name) {
        throw new Error('cookie.getSync must be compiled');
    },
    async get(_name) {
        throw new Error('cookie.get must be compiled');
    },
    async set(_name, _value, _options = {}) {
        throw new Error('cookie.set must be compiled');
    },
    async delete(_name, _options = {}) {
        throw new Error('cookie.delete must be compiled');
    },
};
