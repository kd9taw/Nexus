// The world-atlas TopoJSON ships as JSON; declare it as `any` so tsc doesn't try
// to infer a giant literal type for the bundled basemap.
declare module 'world-atlas/countries-110m.json' {
  const topology: unknown
  export default topology
}
