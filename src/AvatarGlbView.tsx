import { useEffect, useRef, useState, type RefObject } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import type { PersonaMouthCue, PersonaViseme } from "./personaSpeech";

type AvatarGlbViewProps = {
  modelDataUrl: string;
  stageRef: RefObject<HTMLDivElement | null>;
};

type SpeechMotion = { level: number; roundness: number; viseme: PersonaViseme; mouthCue: PersonaMouthCue; faceWeights: Float32Array | null };

const ARKIT_BLENDSHAPES = [
  "browDownLeft", "browDownRight", "browInnerUp", "browOuterUpLeft", "browOuterUpRight",
  "cheekPuff", "cheekSquintLeft", "cheekSquintRight", "eyeBlinkLeft", "eyeBlinkRight",
  "eyeLookDownLeft", "eyeLookDownRight", "eyeLookInLeft", "eyeLookInRight", "eyeLookOutLeft", "eyeLookOutRight",
  "eyeLookUpLeft", "eyeLookUpRight", "eyeSquintLeft", "eyeSquintRight", "eyeWideLeft", "eyeWideRight",
  "jawForward", "jawLeft", "jawOpen", "jawRight", "mouthClose", "mouthDimpleLeft", "mouthDimpleRight",
  "mouthFrownLeft", "mouthFrownRight", "mouthFunnel", "mouthLeft", "mouthLowerDownLeft", "mouthLowerDownRight",
  "mouthPressLeft", "mouthPressRight", "mouthPucker", "mouthRight", "mouthRollLower", "mouthRollUpper",
  "mouthShrugLower", "mouthShrugUpper", "mouthSmileLeft", "mouthSmileRight", "mouthStretchLeft", "mouthStretchRight",
  "mouthUpperUpLeft", "mouthUpperUpRight", "noseSneerLeft", "noseSneerRight", "tongueOut",
];

const FACE_MOTION_GAINS: Record<string, number> = {
  cheekPuff: 0,
};

function calmFaceMotion(name: string, value: number) {
  if (!Number.isFinite(value) || value < 0.025) return 0;
  const gain = FACE_MOTION_GAINS[name] ?? (name.startsWith("eyeLook") ? 0.28 : 0.62);
  return Math.max(0, Math.min(0.72, value * gain));
}

function isSpeechBlendshape(name: string) {
  return name.startsWith("jaw") || name.startsWith("mouth") || name === "tongueOut";
}

type MorphTarget = {
  mesh: THREE.Mesh;
  index: number;
};

const MOUTH_CUES: PersonaMouthCue[] = ["sil", "pp", "ff", "th", "dd", "kk", "ch", "ss", "nn", "rr", "aa", "e", "i", "o", "u"];

const MOUTH_CUE_BLENDSHAPES: Record<PersonaMouthCue, string[]> = {
  sil: ["viseme_sil", "viseme_silence", "vrc.v_sil", "mouthSil"],
  pp: ["viseme_pp", "vrc.v_pp", "mouthPP"],
  ff: ["viseme_ff", "vrc.v_ff", "mouthFF"],
  th: ["viseme_th", "vrc.v_th", "mouthTH"],
  dd: ["viseme_dd", "vrc.v_dd", "mouthDD"],
  kk: ["viseme_kk", "vrc.v_kk", "mouthKK"],
  ch: ["viseme_ch", "vrc.v_ch", "mouthCH"],
  ss: ["viseme_ss", "vrc.v_ss", "mouthSS"],
  nn: ["viseme_nn", "vrc.v_nn", "mouthNN"],
  rr: ["viseme_rr", "vrc.v_rr", "mouthRR"],
  aa: ["viseme_aa", "vrc.v_aa", "mouthAA"],
  e: ["viseme_e", "vrc.v_e", "mouthE"],
  i: ["viseme_i", "vrc.v_i", "mouthI"],
  o: ["viseme_o", "vrc.v_o", "mouthO"],
  u: ["viseme_u", "vrc.v_u", "mouthU"],
};

const PHONEME_JAW_OPEN: Record<PersonaMouthCue, number> = {
  sil: 0.008, pp: 0.006, ff: 0.06, th: 0.16, dd: 0.12, kk: 0.15, ch: 0.14, ss: 0.055,
  nn: 0.1, rr: 0.14, aa: 0.72, e: 0.38, i: 0.25, o: 0.4, u: 0.22,
};

function normalizeMorphName(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function decodeGlbDataUrl(dataUrl: string) {
  const separator = dataUrl.indexOf(",");
  if (separator < 0) throw new Error("The local avatar model has an invalid data URL.");
  const binary = window.atob(dataUrl.slice(separator + 1));
  const buffer = new ArrayBuffer(binary.length);
  const bytes = new Uint8Array(buffer);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return buffer;
}

export default function AvatarGlbView({ modelDataUrl, stageRef }: AvatarGlbViewProps) {
  const mountRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState("LOADING LOCAL 3D HEAD");

  useEffect(() => {
    const mount = mountRef.current;
    if (!mount) return;

    let disposed = false;
    let frame = 0;
    let pivot: THREE.Group | null = null;
    let gltfScene: THREE.Group | null = null;
    let shapeMeshes: THREE.Mesh[] = [];
    let renderer: THREE.WebGLRenderer | null = null;
    let resizeObserver: ResizeObserver | null = null;
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(28, 1, 0.01, 100);
    let motion: SpeechMotion = { level: 0, roundness: 0.45, viseme: "neutral", mouthCue: "sil", faceWeights: null };
    const morphTargetsByName = new Map<string, MorphTarget[]>();
    const morphTargetAliasCache = new Map<string, MorphTarget[]>();
    const smoothedFaceWeights = new Float32Array(ARKIT_BLENDSHAPES.length);
    let modelMotionWasActive = false;

    try {
      renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true, powerPreference: "low-power" });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
      renderer.setClearColor(0x000000, 0);
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = 1.15;
      mount.appendChild(renderer.domElement);
    } catch (cause) {
      setStatus(`3D RENDERER UNAVAILABLE · ${cause instanceof Error ? cause.message : String(cause)}`);
      return;
    }

    scene.add(new THREE.HemisphereLight(0xf2f3f5, 0x20242a, 1.85));
    const keyLight = new THREE.DirectionalLight(0xfff2e6, 2.15);
    keyLight.position.set(2.6, 3.4, 4.2);
    scene.add(keyLight);
    const fillLight = new THREE.DirectionalLight(0xdbe3ec, 0.72);
    fillLight.position.set(-3.2, 0.6, 2.1);
    scene.add(fillLight);

    const resize = () => {
      if (!renderer || !mount) return;
      const width = Math.max(1, mount.clientWidth);
      const height = Math.max(1, mount.clientHeight);
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
    };
    resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(mount);
    resize();

    const onMouthMotion = (event: Event) => {
      const custom = event as CustomEvent<SpeechMotion>;
      if (!custom.detail) return;
      motion = {
        level: Math.max(0, Math.min(1, Number(custom.detail.level) || 0)),
        roundness: Math.max(0, Math.min(1, Number(custom.detail.roundness) || 0)),
        viseme: ["open", "wide", "round", "closed", "neutral"].includes(custom.detail.viseme) ? custom.detail.viseme : "neutral",
        mouthCue: MOUTH_CUES.includes(custom.detail.mouthCue) ? custom.detail.mouthCue : "sil",
        faceWeights: custom.detail.faceWeights?.length === ARKIT_BLENDSHAPES.length ? custom.detail.faceWeights : null,
      };
    };
    const stage = stageRef.current;
    stage?.addEventListener("jadx-avatar-mouth", onMouthMotion);

    const loader = new GLTFLoader();
    try {
      loader.parse(decodeGlbDataUrl(modelDataUrl), "", (gltf) => {
        if (disposed) {
          disposeScene(gltf.scene);
          return;
        }

        gltfScene = gltf.scene;
        pivot = new THREE.Group();
        pivot.add(gltfScene);
        scene.add(pivot);

        const bounds = new THREE.Box3().setFromObject(gltfScene);
        const size = bounds.getSize(new THREE.Vector3());
        const center = bounds.getCenter(new THREE.Vector3());
        gltfScene.position.sub(center);
        const height = Math.max(size.y, size.x * 0.82, size.z * 0.82, 0.1);
        const verticalFit = height / (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)));
        const horizontalFit = (height * 0.72) / (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * Math.max(camera.aspect, 0.5));
        camera.position.set(0, 0, Math.max(verticalFit, horizontalFit) * 1.35);
        camera.lookAt(0, 0, 0);
        camera.near = Math.max(0.001, camera.position.z / 1000);
        camera.far = camera.position.z * 20;
        camera.updateProjectionMatrix();

        const targetNames = new Set<string>();
        gltfScene.traverse((object) => {
          const mesh = object as THREE.Mesh;
          if (!mesh.isMesh || !mesh.morphTargetDictionary || !mesh.morphTargetInfluences) return;
          shapeMeshes.push(mesh);
          Object.entries(mesh.morphTargetDictionary).forEach(([name, index]) => {
            const normalized = normalizeMorphName(name);
            targetNames.add(normalized);
            const targets = morphTargetsByName.get(normalized) ?? [];
            targets.push({ mesh, index });
            morphTargetsByName.set(normalized, targets);
          });
        });

        // The render loop runs while GLTFLoader is still parsing and may cache
        // empty alias lookups. Drop those misses now that the morph dictionary
        // has been populated so status detection and speech animation see the
        // newly loaded face targets.
        morphTargetAliasCache.clear();

        const hasMouthKey = ["jawOpen", "mouthOpen", "mouthClose", "mouthFunnel", "mouthPucker", "mouthStretchLeft", "mouthStretchRight"]
          .some((name) => hasTargets([name])) || MOUTH_CUES.some((cue) => hasTargets(MOUTH_CUE_BLENDSHAPES[cue]));
        const targetCount = targetNames.size;
        const mouthCueCount = MOUTH_CUES.filter((cue) => hasTargets(MOUTH_CUE_BLENDSHAPES[cue])).length;
        const arkitCount = ARKIT_BLENDSHAPES.filter((name) => hasTargets([name])).length;
        setStatus(hasMouthKey || mouthCueCount > 0
          ? `LIP SYNC READY · ${targetCount} MORPHS · ${mouthCueCount} VISEMES · ${arkitCount} ARKIT`
          : targetCount > 0
            ? `MODEL HAS ${targetCount} MORPHS · NO RECOGNIZED MOUTH SHAPES`
            : "NO LIP SHAPES · EXPORT WITH ARKIT BLENDSHAPES OR VISEMES");
      }, (cause) => {
        if (!disposed) setStatus(`COULD NOT READ LOCAL GLB · ${cause instanceof Error ? cause.message : String(cause)}`);
      });
    } catch (cause) {
      setStatus(`COULD NOT READ LOCAL GLB · ${cause instanceof Error ? cause.message : String(cause)}`);
    }

    const targetsForAlias = (alias: string) => {
      const normalized = normalizeMorphName(alias);
      const cached = morphTargetAliasCache.get(normalized);
      if (cached) return cached;
      const exact = morphTargetsByName.get(normalized);
      const matches = exact ?? [...morphTargetsByName.entries()]
        .filter(([name]) => name.endsWith(normalized))
        .flatMap(([, targets]) => targets);
      morphTargetAliasCache.set(normalized, matches);
      return matches;
    };
    const setTargets = (aliases: string[], value: number) => {
      for (const alias of aliases) {
        for (const target of targetsForAlias(alias)) {
          const influences = target.mesh.morphTargetInfluences;
          if (influences) influences[target.index] = value;
        }
      }
    };
    const hasTargets = (aliases: string[]) => aliases.some((alias) => targetsForAlias(alias).length > 0);

    let nextBlinkAt = performance.now() + 2800 + Math.random() * 2400;
    let blinkStartedAt = 0;
    let previousFrameAt = 0;
    const mouthCueWeights = Object.fromEntries(MOUTH_CUES.map((cue) => [cue, cue === "sil" ? 1 : 0])) as Record<PersonaMouthCue, number>;
    const animate = (timestamp: number) => {
      if (disposed || !renderer) return;
      const seconds = timestamp / 1000;
      if (pivot) {
        pivot.rotation.y = Math.sin(seconds * 0.28) * 0.085;
        pivot.rotation.x = Math.sin(seconds * 0.34 + 0.7) * 0.018;
        pivot.rotation.z = Math.sin(seconds * 0.22 + 1.3) * 0.009;
      }

      if (!blinkStartedAt && timestamp >= nextBlinkAt) blinkStartedAt = timestamp;
      let blink = 0;
      if (blinkStartedAt) {
        const elapsed = timestamp - blinkStartedAt;
        if (elapsed < 90) blink = elapsed / 90;
        else if (elapsed < 135) blink = 1;
        else if (elapsed < 245) blink = 1 - (elapsed - 135) / 110;
        else {
          blinkStartedAt = 0;
          nextBlinkAt = timestamp + 2900 + Math.random() * 2700;
        }
      }
      setTargets(["eyeBlinkLeft"], blink * 0.88);
      setTargets(["eyeBlinkRight"], blink * 0.88);

      const level = motion.level;
      const roundness = motion.roundness;
      const deltaSeconds = previousFrameAt ? Math.max(0.001, (timestamp - previousFrameAt) / 1000) : 1 / 60;
      previousFrameAt = timestamp;
      const energy = Math.max(0, Math.min(1, level * 1.65));
      const faceWeights = motion.faceWeights;
      if (faceWeights) {
        modelMotionWasActive = true;
        for (let index = 0; index < ARKIT_BLENDSHAPES.length; index += 1) {
          const name = ARKIT_BLENDSHAPES[index];
          // The audio model's mouth coefficients are too energetic for this
          // rig. Keep its upper-face expression, and drive speech from timed
          // visemes below so jaw/lip motion stays restrained and text-aligned.
          if (isSpeechBlendshape(name)) continue;
          const modelValue = calmFaceMotion(name, faceWeights[index] ?? 0);
          const value = Math.max(modelValue, name === "eyeBlinkLeft" || name === "eyeBlinkRight" ? blink * 0.88 : 0);
          const responseSeconds = value > smoothedFaceWeights[index] ? 0.11 : 0.17;
          const blend = 1 - Math.exp(-deltaSeconds / responseSeconds);
          smoothedFaceWeights[index] += (value - smoothedFaceWeights[index]) * blend;
          setTargets([name], smoothedFaceWeights[index]);
        }
      }
      if (!faceWeights && modelMotionWasActive) {
        let hasResidual = false;
        for (let index = 0; index < ARKIT_BLENDSHAPES.length; index += 1) {
          const name = ARKIT_BLENDSHAPES[index];
          if (isSpeechBlendshape(name) || name === "eyeBlinkLeft" || name === "eyeBlinkRight") {
            smoothedFaceWeights[index] = 0;
            continue;
          }
          const blend = 1 - Math.exp(-deltaSeconds / 0.16);
          smoothedFaceWeights[index] += (0 - smoothedFaceWeights[index]) * blend;
          if (smoothedFaceWeights[index] > 0.001) hasResidual = true;
          setTargets([name], smoothedFaceWeights[index]);
        }
        modelMotionWasActive = hasResidual;
      }
      const mouthCueBlend = 1 - Math.exp(-deltaSeconds / 0.09);
      for (const cue of MOUTH_CUES) {
        const target = motion.mouthCue === cue ? 1 : 0;
        mouthCueWeights[cue] += (target - mouthCueWeights[cue]) * mouthCueBlend;
        const aliases = MOUTH_CUE_BLENDSHAPES[cue];
        if (hasTargets(aliases)) {
          const vowel = cue === "aa" || cue === "e" || cue === "i" || cue === "o" || cue === "u";
          const strength = cue === "sil" ? 0.06 : (vowel ? 0.34 : 0.27) * (0.82 + energy * 0.18);
          setTargets(aliases, mouthCueWeights[cue] * strength);
        }
      }

      const fallbackWeight = (cue: PersonaMouthCue) => hasTargets(MOUTH_CUE_BLENDSHAPES[cue]) ? 0.22 : 1;
      const cueWeight = (cue: PersonaMouthCue) => mouthCueWeights[cue] * fallbackWeight(cue);
      const jawProfile = MOUTH_CUES.reduce((sum, cue) => sum + cueWeight(cue) * PHONEME_JAW_OPEN[cue], 0);
      const audioJawMotion = energy * (motion.viseme === "closed" ? 0.018 : 0.055);
      const jawOpen = Math.max(0, Math.min(0.46, jawProfile * (0.82 + energy * 0.34) + audioJawMotion));
      const pp = cueWeight("pp");
      const ff = cueWeight("ff");
      const th = cueWeight("th");
      const ch = cueWeight("ch");
      const ss = cueWeight("ss");
      const rr = cueWeight("rr");
      const openO = cueWeight("o");
      const openU = cueWeight("u");
      const wideE = cueWeight("e");
      const wideI = cueWeight("i");
      const roundShape = Math.max(0, Math.min(1, openO * 0.72 + openU * 0.8 + rr * 0.18 + ch * 0.12 + energy * roundness * 0.08));
      const closeShape = Math.max(0, Math.min(0.46, pp * 0.62 + ff * 0.09));
      const wideAmount = Math.max(0, Math.min(0.2, wideE * 0.12 + wideI * 0.2 + ss * 0.06 + energy * (1 - roundness) * 0.025));
      setTargets(["jawOpen"], jawOpen);
      setTargets(["mouthClose"], closeShape);
      setTargets(["mouthPressLeft", "mouthPressRight"], pp * 0.22);
      setTargets(["mouthLowerDownLeft", "mouthLowerDownRight"], jawOpen * 0.06 + th * 0.06);
      setTargets(["mouthUpperUpLeft", "mouthUpperUpRight"], ff * 0.08);
      setTargets(["mouthRollLower", "mouthRollUpper", "tongueOut"], 0);
      setTargets(["mouthFunnel"], roundShape * 0.28);
      setTargets(["mouthPucker"], openO * 0.16 + openU * 0.23 + rr * 0.03 + ch * 0.02);
      setTargets(["mouthDimpleLeft", "mouthDimpleRight"], ss * 0.06);
      setTargets(["mouthSmileLeft", "mouthSmileRight"], (wideE * 0.03 + wideI * 0.04));
      setTargets(["mouthStretchLeft"], wideAmount);
      setTargets(["mouthStretchRight"], wideAmount);

      renderer.render(scene, camera);
      frame = window.requestAnimationFrame(animate);
    };
    frame = window.requestAnimationFrame(animate);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(frame);
      resizeObserver?.disconnect();
      stage?.removeEventListener("jadx-avatar-mouth", onMouthMotion);
      if (gltfScene) disposeScene(gltfScene);
      renderer?.dispose();
      renderer?.domElement.remove();
      renderer = null;
      shapeMeshes = [];
    };
  }, [modelDataUrl, stageRef]);

  return (
    <div className="avatar-glb-view" aria-label="Locally rendered 3D face avatar">
      <div className="avatar-glb-canvas" ref={mountRef} />
      <div className="avatar-glb-status" role="status">{status}</div>
    </div>
  );
}

function disposeScene(root: THREE.Object3D) {
  root.traverse((object) => {
    const mesh = object as THREE.Mesh;
    if (!mesh.isMesh) return;
    mesh.geometry?.dispose();
    const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
    for (const material of materials) {
      for (const value of Object.values(material)) {
        if (value instanceof THREE.Texture) value.dispose();
      }
      material.dispose();
    }
  });
}
