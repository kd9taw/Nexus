// The 3-D globes' base map: the lines sit just above three-globe's sphere and carry exactly the
// data's segments, and the city lights are masked to the night side in the shader three.js really
// compiles (a renamed shader chunk would leave the replace a silent no-op; this would catch it).
import { describe, expect, it } from 'vitest'
import * as THREE from 'three'
import { coarsestBasemap } from '../basemap'
import { globeLines, nightOnlyEmissive } from './globeBasemap'

const INKS = { land: '#111111', water: '#222222', river: '#333333', coast: '#444444', state: '#555555' }

describe('globeLines', () => {
  const map = coarsestBasemap()
  const g = globeLines(map, INKS)
  const positions = (name: string) => ((g.getObjectByName(name) as THREE.LineSegments).geometry.getAttribute('position').array as Float32Array)

  it('holds the coast, the borders and the US states, each its own object', () => {
    expect(g.children.map((c) => c.name).sort()).toEqual(['borders', 'coast', 'states'])
  })

  it('puts every vertex a hair above the sphere of radius 100', () => {
    for (const name of ['coast', 'borders', 'states']) {
      const p = positions(name)
      expect(p.length).toBeGreaterThan(0)
      for (let i = 0; i < p.length; i += 3) {
        const r = Math.hypot(p[i], p[i + 1], p[i + 2])
        if (Math.abs(r - 100.15) > 1e-3) throw new Error(`${name}: vertex at radius ${r}`)
      }
    }
  })

  it('draws exactly the shore runs as segments (a closed run closes)', () => {
    let segments = 0
    for (const t of [...map.land.tiles, ...map.lakes.tiles]) {
      for (let k = 0; k < t.shore!.length; k += 3) segments += t.shore![k + 1] - 1 + t.shore![k + 2]
    }
    expect(positions('coast').length).toBe(segments * 6)
  })

  it('places lat/lon where three-globe does (north pole up, lon 0 towards +z)', () => {
    // three-globe polar2Cartesian: lat 90 → +y; lat 0, lon 0 → +z; lat 0, lon 90 → +x.
    const one = { ...map, land: { tiles: [] }, lakes: { tiles: [] }, rivers: { tiles: [] }, borders: { tiles: [] } }
    const t = { index: 0, west: -180, south: -90, east: 180, north: 90, coords: Float32Array.from([0, 0, 90, 0]), parts: Uint32Array.from([0, 2]) }
    const p = (globeLines({ ...one, states: { tiles: [t] } }, INKS).getObjectByName('states') as THREE.LineSegments).geometry.getAttribute('position').array
    expect([...p].map((v) => Math.round(v * 100) / 100 + 0)).toEqual([0, 0, 100.15, 100.15, 0, 0])
  })
})

describe('nightOnlyEmissive', () => {
  it("masks the emissive by the sun's angle in three.js's own Phong shader", () => {
    const mat = new THREE.MeshPhongMaterial()
    nightOnlyEmissive(mat)
    const shader = { fragmentShader: THREE.ShaderLib.phong.fragmentShader, vertexShader: '', uniforms: {} }
    expect(shader.fragmentShader, 'CONTROL: the chunk the mask hooks onto exists').toContain('#include <emissivemap_fragment>')
    mat.onBeforeCompile(shader as unknown as THREE.WebGLProgramParametersWithUniforms, {} as THREE.WebGLRenderer)
    expect(shader.fragmentShader).toContain('totalEmissiveRadiance *= 1.0 - smoothstep( -0.12, 0.08, dot( normal, directionalLights[ 0 ].direction ) );')
    // ...after the emissive is read, before the lights are summed.
    const src = shader.fragmentShader
    expect(src.indexOf('totalEmissiveRadiance *=')).toBeGreaterThan(src.indexOf('#include <emissivemap_fragment>'))
    expect(src.indexOf('totalEmissiveRadiance *=')).toBeLessThan(src.indexOf('#include <lights_phong_fragment>'))
  })
})
