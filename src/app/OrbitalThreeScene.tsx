import { useEffect, useRef, useState } from 'react';
import type { OrbitalVisualization } from '../shared/contracts';

type Props = {
  visualization: OrbitalVisualization;
  reduceMotion: boolean;
  paused: boolean;
  rotationSpeed: 0.5 | 1 | 1.5;
  particlesEnabled: boolean;
  intensity: 'low' | 'balanced' | 'high';
  onStatus: (status: 'loading' | 'ready' | 'error') => void;
};

/** Lazy, low-power orbital visuals. Each selected mode uses a distinct Three scene; the authored SVG remains the no-WebGL fallback. */
export function OrbitalThreeScene({ visualization, reduceMotion, paused, rotationSpeed, particlesEnabled, intensity, onStatus }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [canvasReady, setCanvasReady] = useState(false);
  const frameRef = useRef(0);
  const renderLoopRef = useRef<((time: number) => void) | null>(null);
  const controlsRef = useRef({ reduceMotion, paused, rotationSpeed });
  controlsRef.current = { reduceMotion, paused, rotationSpeed };
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches);

  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setPrefersReducedMotion(media.matches);
    update(); media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  useEffect(() => {
    const mayAnimate = !reduceMotion && !prefersReducedMotion && !paused;
    if (!mayAnimate) cancelAnimationFrame(frameRef.current);
    else if (renderLoopRef.current) frameRef.current = requestAnimationFrame(renderLoopRef.current);
  }, [paused, prefersReducedMotion, reduceMotion]);

  useEffect(() => {
    let disposed = false;
    let lastFrame = 0;
    let renderer: import('three').WebGLRenderer | undefined;
    let scene: import('three').Scene | undefined;
    let camera: import('three').PerspectiveCamera | undefined;
    let planet: import('three').Object3D | undefined;
    let orbitalGroup: import('three').Group | undefined;
    let observer: ResizeObserver | undefined;
    let contextLost = false;
    const resources: Array<{ dispose: () => void }> = [];
    const canvas = canvasRef.current;
    setCanvasReady(false);
    onStatus('loading');
    const handleContextLost = (event: Event) => {
      event.preventDefault(); contextLost = true; cancelAnimationFrame(frameRef.current); setCanvasReady(false); onStatus('error');
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') cancelAnimationFrame(frameRef.current);
      else if (!controlsRef.current.reduceMotion && !controlsRef.current.paused && !prefersReducedMotion && renderLoopRef.current) frameRef.current = requestAnimationFrame(renderLoopRef.current);
    };
    canvas?.addEventListener('webglcontextlost', handleContextLost);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    const glow = intensity === 'high' ? 1.35 : intensity === 'low' ? .62 : 1;

    const boot = async () => {
      try {
        const THREE = await import('three');
        if (disposed) return;
        if (!canvas) { onStatus('error'); return; }
        const css = (token: string, fallback: string) => getComputedStyle(document.documentElement).getPropertyValue(token).trim() || fallback;
        const plasma = new THREE.Color(css('--color-data-primary', '#a58aff'));
        const brass = new THREE.Color(css('--color-data-secondary', '#e6b47a'));
        const mint = new THREE.Color(css('--color-action', '#75ddd0'));
        const ocean = css('--earth-ocean', '#101f52');
        const land = css('--earth-land', '#3659a2');
        const landLight = css('--earth-land-lit', '#5979c2');
        const cloud = css('--earth-cloud', '#c9e5ff');

        renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: 'low-power' });
        renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
        renderer.outputColorSpace = THREE.SRGBColorSpace;
        renderer.setClearColor(0x000000, 0);
        scene = new THREE.Scene();
        camera = new THREE.PerspectiveCamera(36, 1, 0.1, 100);
        camera.position.set(0, 0.12, 5.1);
        scene.add(new THREE.HemisphereLight(0xb8c8ff, 0x100d27, 1.2));
        const keyLight = new THREE.DirectionalLight(0xd5d4ff, 2.15);
        keyLight.position.set(-3, 3, 4);
        const rimLight = new THREE.PointLight(mint, 1.45, 9);
        rimLight.position.set(3, 0.7, 2.5);
        scene.add(keyLight, rimLight);
        if (particlesEnabled) addStarfield(THREE, scene, resources, intensity === 'high' ? 210 : intensity === 'low' ? 62 : 120, intensity === 'low' ? .32 : .66);

        const sphere = new THREE.SphereGeometry(1.36, 80, 56);
        resources.push(sphere);
        orbitalGroup = new THREE.Group();
        scene.add(orbitalGroup);

        if (visualization === 'earth') {
          const textureCanvas = document.createElement('canvas');
          textureCanvas.width = 1536;
          textureCanvas.height = 768;
          const ctx = textureCanvas.getContext('2d');
          if (!ctx) throw new Error('Canvas texture unavailable');
          const oceanGradient = ctx.createLinearGradient(0, 0, 1536, 768);
          oceanGradient.addColorStop(0, css('--earth-ocean-light', '#142c69'));
          oceanGradient.addColorStop(.52, ocean);
          oceanGradient.addColorStop(1, css('--earth-ocean-shadow', '#091334'));
          ctx.fillStyle = oceanGradient;
          ctx.fillRect(0, 0, 1536, 768);

          // Irregular cubic coastlines, not polygonal icon shapes. The curves create broad landmasses at globe scale.
          const continents = [
            'M190 145 C219 99 276 72 329 91 C355 100 367 120 394 123 C419 127 433 148 417 169 C400 188 382 184 372 212 C363 236 338 246 318 266 C296 288 292 327 262 337 C231 347 215 323 209 295 C203 271 175 258 167 232 C157 202 179 184 166 163 C158 151 171 144 190 145Z',
            'M345 342 C369 331 394 339 410 359 C428 382 427 410 443 433 C463 462 456 507 435 546 C418 576 414 619 389 660 C374 686 351 672 346 643 C340 608 321 580 320 546 C319 512 302 487 304 452 C306 418 323 398 315 374 C309 356 326 345 345 342Z',
            'M561 124 C592 94 643 84 686 95 C716 102 734 126 770 126 C805 125 833 143 858 164 C884 185 919 191 930 222 C939 250 912 270 882 268 C854 266 843 284 826 305 C807 328 780 319 758 311 C733 301 717 319 697 341 C678 362 649 360 637 339 C623 315 598 315 581 298 C558 276 563 250 540 232 C522 218 515 194 531 177 C548 160 539 143 561 124Z',
            'M714 340 C744 323 782 328 805 349 C830 373 838 407 826 440 C813 470 802 498 784 527 C763 562 751 603 724 619 C704 630 687 607 685 580 C683 550 665 525 659 495 C653 465 664 442 654 416 C645 390 675 363 714 340Z',
            'M996 507 C1020 487 1052 492 1077 508 C1100 523 1125 530 1132 552 C1140 577 1111 591 1089 600 C1064 610 1042 630 1014 623 C987 617 969 593 969 568 C969 545 979 523 996 507Z',
          ];
          const landGradient = ctx.createLinearGradient(0, 95, 0, 680);
          landGradient.addColorStop(0, landLight);
          landGradient.addColorStop(.42, land);
          landGradient.addColorStop(1, '#263a73');
          ctx.fillStyle = landGradient;
          ctx.strokeStyle = 'rgba(181, 203, 255, .32)';
          ctx.lineWidth = 2;
          for (const shape of continents) {
            const path = new Path2D(shape);
            ctx.fill(path);
            ctx.stroke(path);
          }
          // Fine, broad coast-adjacent terrain contours add scale without a repeated surface motif.
          ctx.strokeStyle = 'rgba(188, 205, 255, .13)';
          ctx.lineWidth = 1.3;
          for (let index = 0; index < continents.length; index++) {
            const x = 200 + index * 145;
            ctx.beginPath();
            ctx.bezierCurveTo(x, 200 + index * 8, x + 30, 238 + index * 9, x - 5, 276 + index * 7);
            ctx.stroke();
          }
          ctx.strokeStyle = `${cloud}34`;
          ctx.lineWidth = 15;
          ctx.lineCap = 'round';
          for (const [x, y, width] of [[90, 245, 300], [427, 109, 370], [404, 574, 430], [1010, 274, 315]]) {
            ctx.beginPath();
            ctx.moveTo(x, y);
            ctx.bezierCurveTo(x + width * .26, y - 35, x + width * .68, y + 40, x + width, y - 7);
            ctx.stroke();
          }
          const texture = new THREE.CanvasTexture(textureCanvas);
          texture.colorSpace = THREE.SRGBColorSpace;
          texture.wrapS = THREE.RepeatWrapping;
          resources.push(texture);
          const material = new THREE.MeshStandardMaterial({ color: 0xffffff, map: texture, roughness: .86, metalness: .025, emissive: ocean, emissiveIntensity: .19 });
          planet = new THREE.Mesh(sphere, material);
          resources.push(material);
          scene.add(planet);
          addAtmosphere(THREE, scene, resources, 1.39, mint);
          addOrbit(THREE, orbitalGroup, resources, 1.78, plasma, 1.2, -.18, .78 * glow);
          addOrbit(THREE, orbitalGroup, resources, 1.62, brass, 1.49, .38, .62 * glow);
        } else if (visualization === 'nebula') {
          const cloudCanvas = document.createElement('canvas');
          cloudCanvas.width = 1024;
          cloudCanvas.height = 512;
          const ctx = cloudCanvas.getContext('2d');
          if (!ctx) throw new Error('Canvas texture unavailable');
          ctx.fillStyle = '#130f2b';
          ctx.fillRect(0, 0, 1024, 512);
          const clouds = [
            [260, 212, 255, 110, '#9a56e8', .88], [450, 160, 205, 105, '#e088dd', .74],
            [615, 275, 265, 125, '#7452d8', .86], [520, 355, 220, 88, '#efacbd', .62],
            [315, 340, 180, 74, '#49b9aa', .48], [735, 160, 135, 60, '#f1c174', .56],
          ] as const;
          for (const [x, y, rx, ry, color, alpha] of clouds) {
            ctx.save();
            ctx.translate(x, y);
            ctx.scale(rx / ry, 1);
            const gradient = ctx.createRadialGradient(0, 0, 2, 0, 0, ry);
            gradient.addColorStop(0, `${color}${Math.round(alpha * 255).toString(16).padStart(2, '0')}`);
            gradient.addColorStop(.56, `${color}${Math.round(alpha * .68 * 255).toString(16).padStart(2, '0')}`);
            gradient.addColorStop(1, `${color}00`);
            ctx.fillStyle = gradient;
            ctx.beginPath(); ctx.arc(0, 0, ry, 0, Math.PI * 2); ctx.fill();
            ctx.restore();
          }
          for (let i = 0; i < 48; i++) {
            const x = (i * 197 + 83) % 1024;
            const y = (i * 113 + 29) % 512;
            ctx.fillStyle = i % 5 === 0 ? 'rgba(255,218,164,.58)' : 'rgba(202,198,255,.45)';
            ctx.beginPath(); ctx.arc(x, y, i % 7 === 0 ? 1.7 : .8, 0, Math.PI * 2); ctx.fill();
          }
          const texture = new THREE.CanvasTexture(cloudCanvas);
          texture.colorSpace = THREE.SRGBColorSpace;
          resources.push(texture);
          const coreMaterial = new THREE.MeshBasicMaterial({ map: texture, transparent: true, side: THREE.FrontSide });
          const core = new THREE.Mesh(sphere, coreMaterial);
          planet = core;
          resources.push(coreMaterial);
          scene.add(core);
          const innerGlow = new THREE.Mesh(new THREE.SphereGeometry(1.08, 48, 36), new THREE.MeshBasicMaterial({ color: plasma, transparent: true, opacity: .13, side: THREE.BackSide }));
          resources.push(innerGlow.geometry, innerGlow.material);
          scene.add(innerGlow);
          addOrbit(THREE, orbitalGroup, resources, 1.77, plasma, 1.2, -.23, .8);
          addOrbit(THREE, orbitalGroup, resources, 1.51, mint, 1.48, .42, .5);
          addOrbit(THREE, orbitalGroup, resources, 1.91, brass, 1.62, -.5, .4);
        } else if (visualization === 'ringed-planet') {
          const surfaceCanvas = document.createElement('canvas'); surfaceCanvas.width = 1024; surfaceCanvas.height = 512;
          const ctx = surfaceCanvas.getContext('2d'); if (!ctx) throw new Error('Canvas texture unavailable');
          const bands = ['#674834','#c38d5d','#e4c291','#a66d43','#ead5a5','#8c5d3e','#d0a16d'];
          ctx.fillStyle = bands[0]; ctx.fillRect(0,0,1024,512);
          bands.forEach((color,index) => { ctx.fillStyle = color; ctx.globalAlpha = .82; ctx.beginPath(); ctx.ellipse(520, 55 + index * 73, 620, 19 + (index % 3) * 8, .01, 0, Math.PI*2); ctx.fill(); }); ctx.globalAlpha=1;
          for (let i=0;i<18;i++) { ctx.strokeStyle = `rgba(255,238,203,${.12 + (i%3)*.04})`; ctx.lineWidth=2; ctx.beginPath(); ctx.moveTo(80+i*49,0); ctx.bezierCurveTo(50+i*50,170,100+i*47,310,55+i*51,512); ctx.stroke(); }
          const texture = new THREE.CanvasTexture(surfaceCanvas); texture.colorSpace = THREE.SRGBColorSpace; texture.wrapS = THREE.RepeatWrapping; resources.push(texture);
          const material = new THREE.MeshStandardMaterial({ map:texture, roughness:.7, metalness:.06, emissive:'#3a2114', emissiveIntensity:.18 }); resources.push(material);
          planet = new THREE.Mesh(sphere,material); scene.add(planet); addAtmosphere(THREE,scene,resources,1.39,new THREE.Color('#e5b574'));
          const ring = new THREE.Mesh(new THREE.RingGeometry(1.55,2.42,160), new THREE.MeshStandardMaterial({ color:'#d8bf9a', side:THREE.DoubleSide, transparent:true, opacity:.68, roughness:.85, metalness:.15 })); ring.rotation.x=1.27; ring.rotation.y=.16; ring.position.z=-.08; scene.add(ring); resources.push(ring.geometry,ring.material);
          const moon = new THREE.Mesh(new THREE.SphereGeometry(.12,24,16),new THREE.MeshStandardMaterial({color:'#d8d0bb',roughness:1})); moon.position.set(2.15,.72,.25); scene.add(moon); resources.push(moon.geometry,moon.material);
          addOrbit(THREE,orbitalGroup,resources,2.55,mint,1.24,.4,.45*glow);
        } else if (visualization === 'lunar-station') {
          const moonCanvas=document.createElement('canvas'); moonCanvas.width=1024; moonCanvas.height=512; const ctx=moonCanvas.getContext('2d'); if(!ctx) throw new Error('Canvas texture unavailable');
          ctx.fillStyle='#777d82';ctx.fillRect(0,0,1024,512); const seed=(n:number)=>{const v=Math.sin(n*127.1+311.7)*43758.5453;return v-Math.floor(v);};
          for(let i=0;i<120;i++){const x=seed(i)*1024,y=seed(i+140)*512,r=3+seed(i+280)*22;ctx.fillStyle=`rgba(27,32,37,${.12+seed(i+420)*.24})`;ctx.beginPath();ctx.ellipse(x,y,r*1.2,r,0,0,Math.PI*2);ctx.fill();ctx.strokeStyle='rgba(225,228,219,.12)';ctx.lineWidth=2;ctx.stroke();}
          const texture=new THREE.CanvasTexture(moonCanvas);texture.colorSpace=THREE.SRGBColorSpace;texture.wrapS=THREE.RepeatWrapping;resources.push(texture);
          const material=new THREE.MeshStandardMaterial({map:texture,roughness:1});resources.push(material);planet=new THREE.Mesh(sphere,material);scene.add(planet);
          const station=new THREE.Group();const metal=new THREE.MeshStandardMaterial({color:'#d8e2df',metalness:.72,roughness:.34});const solar=new THREE.MeshStandardMaterial({color:'#3f8d9a',metalness:.38,roughness:.5,emissive:'#123942',emissiveIntensity:.12});resources.push(metal,solar);
          const body=new THREE.Mesh(new THREE.CylinderGeometry(.09,.09,.34,12),metal);body.rotation.z=Math.PI/2;station.add(body);
          for(const side of [-1,1]){const panel=new THREE.Mesh(new THREE.BoxGeometry(.52,.015,.22),solar);panel.position.x=side*.38;station.add(panel);}
          station.position.set(1.72,.8,.22);station.rotation.z=-.25;scene.add(station);planet.add(station); resources.push(body.geometry);station.traverse(child=>{if(child instanceof THREE.Mesh)resources.push(child.geometry);});
          addOrbit(THREE,orbitalGroup,resources,1.88,brass,1.08,-.2,.52*glow);
        } else if (visualization === 'core') {
          const coreMat=new THREE.MeshBasicMaterial({color:plasma,transparent:true,opacity:.92});const shellMat=new THREE.MeshBasicMaterial({color:mint,wireframe:true,transparent:true,opacity:.24});resources.push(coreMat,shellMat);
          const core=new THREE.Mesh(new THREE.IcosahedronGeometry(.76,4),coreMat);planet=core;scene.add(core);
          const shell=new THREE.Mesh(new THREE.IcosahedronGeometry(1.16,2),shellMat);shell.rotation.set(.4,.2,.1);scene.add(shell);resources.push(shell.geometry);
          const halo=new THREE.Mesh(new THREE.SphereGeometry(1.45,40,28),new THREE.MeshBasicMaterial({color:plasma,transparent:true,opacity:.08*glow,side:THREE.BackSide}));scene.add(halo);resources.push(halo.geometry,halo.material);
          addOrbit(THREE,orbitalGroup,resources,1.72,plasma,1.15,-.18,.8*glow);addOrbit(THREE,orbitalGroup,resources,1.92,mint,.68,.52,.55*glow);addOrbit(THREE,orbitalGroup,resources,1.53,brass,1.51,-.4,.44*glow);
        } else if (visualization === 'constellations') {
          const points = [[-1.2,.65,.35],[-.65,.9,-.25],[-.12,.35,.65],[.55,.85,.15],[1.1,.28,-.5],[.75,-.5,.55],[.05,-.9,.1],[-.85,-.42,-.35]] as const;
          const geometry=new THREE.BufferGeometry().setFromPoints(points.map(([x,y,z])=>new THREE.Vector3(x*1.28,y*1.28,z*1.28)));const material=new THREE.LineBasicMaterial({color:mint,transparent:true,opacity:.72*glow});const constellation=new THREE.Line(new THREE.BufferGeometry().setFromPoints([points[0],points[1],points[2],points[3],points[4],points[5],points[6],points[7]].map(([x,y,z])=>new THREE.Vector3(x*1.28,y*1.28,z*1.28))),material);scene.add(constellation);resources.push(geometry,constellation.geometry,material);
          const stars=new THREE.Points(geometry,new THREE.PointsMaterial({color:'#fff0c4',size:.055*glow,sizeAttenuation:true}));scene.add(stars);resources.push(stars.material);
          for(const [x,y,z] of points){const node=new THREE.Mesh(new THREE.SphereGeometry(.045,12,8),new THREE.MeshBasicMaterial({color:brass}));node.position.set(x*1.28,y*1.28,z*1.28);scene.add(node);resources.push(node.geometry,node.material);}
          addOrbit(THREE,orbitalGroup,resources,2.05,plasma,1.25,.2,.45*glow);
        } else {
          const wireGlobe = new THREE.Group();
          const wireSphere = new THREE.Mesh(new THREE.SphereGeometry(1.355, 24, 16), new THREE.MeshBasicMaterial({ color: '#252044', transparent: true, opacity: .56 }));
          resources.push(wireSphere.geometry, wireSphere.material);
          wireGlobe.add(wireSphere);
          planet = wireGlobe;
          const wireMaterial = new THREE.LineBasicMaterial({ color: plasma, transparent: true, opacity: .75 });
          const accentMaterial = new THREE.LineBasicMaterial({ color: mint, transparent: true, opacity: .58 });
          resources.push(wireMaterial, accentMaterial);
          // Globe-like parallels and meridians read as a navigational grid rather than a triangulated mesh.
          for (let index = -4; index <= 4; index++) {
            const latitude = index * Math.PI / 10;
            const radius = 1.36 * Math.cos(latitude);
            const y = 1.36 * Math.sin(latitude);
            const points = Array.from({ length: 97 }, (_, step) => {
              const angle = step / 96 * Math.PI * 2;
              return new THREE.Vector3(radius * Math.cos(angle), y, radius * Math.sin(angle));
            });
            const geometry = new THREE.BufferGeometry().setFromPoints(points);
            wireGlobe.add(new THREE.LineLoop(geometry, index === 0 ? accentMaterial : wireMaterial));
            resources.push(geometry);
          }
          for (let index = 0; index < 12; index++) {
            const longitude = index * Math.PI / 12;
            const points = Array.from({ length: 97 }, (_, step) => {
              const angle = step / 96 * Math.PI * 2;
              return new THREE.Vector3(1.36 * Math.cos(angle) * Math.cos(longitude), 1.36 * Math.sin(angle), 1.36 * Math.cos(angle) * Math.sin(longitude));
            });
            const geometry = new THREE.BufferGeometry().setFromPoints(points);
            wireGlobe.add(new THREE.LineLoop(geometry, index % 3 === 0 ? accentMaterial : wireMaterial));
            resources.push(geometry);
          }
          scene.add(wireGlobe);
          for (const [radius, rotation, color] of [[1.82, 1.18, brass], [1.58, 1.49, mint]] as const) addOrbit(THREE, orbitalGroup, resources, radius, color, rotation, .25, .7);
        }

        observer = new ResizeObserver(() => {
          if (!renderer || !canvas || !camera || !scene) return;
          const bounds = canvas.getBoundingClientRect();
          if (!bounds.width || !bounds.height) return;
          renderer.setSize(bounds.width, bounds.height, false);
          camera.aspect = bounds.width / bounds.height;
          camera.updateProjectionMatrix();
          renderer.render(scene, camera);
        });
        observer.observe(canvas);
        const bounds = canvas.getBoundingClientRect();
        renderer.setSize(bounds.width, bounds.height, false);
        camera.aspect = bounds.width / Math.max(1, bounds.height);
        camera.updateProjectionMatrix();
        renderer.render(scene, camera);
        setCanvasReady(true); onStatus('ready');
        const render = (time: number) => {
          if (disposed || contextLost || !renderer || !scene || !camera) return;
          if (!lastFrame || time - lastFrame >= 33) {
            const delta = lastFrame ? Math.min(time - lastFrame, 100) / 1000 : 0;
            if (planet && orbitalGroup) {
              planet.rotation.y += delta * (visualization === 'nebula' ? .075 : .035) * controlsRef.current.rotationSpeed;
              orbitalGroup.rotation.z += delta * .009 * controlsRef.current.rotationSpeed;
              if (visualization === 'nebula') planet.rotation.x = Math.sin(time / 3600) * .055;
            }
            renderer.render(scene, camera);
            lastFrame = time;
          }
          if (document.visibilityState === 'visible' && !controlsRef.current.reduceMotion && !prefersReducedMotion && !controlsRef.current.paused) frameRef.current = requestAnimationFrame(render);
        };
        renderLoopRef.current = render;
        if (document.visibilityState === 'visible' && !controlsRef.current.reduceMotion && !prefersReducedMotion && !controlsRef.current.paused) frameRef.current = requestAnimationFrame(render);
      } catch {
        cancelAnimationFrame(frameRef.current);
        observer?.disconnect();
        renderer?.dispose();
        resources.forEach(resource => resource.dispose());
        if (!disposed) { setCanvasReady(false); onStatus('error'); }
      }
    };
    void boot();
    return () => {
      disposed = true;
      cancelAnimationFrame(frameRef.current);
      renderLoopRef.current = null;
      observer?.disconnect();
      canvas?.removeEventListener('webglcontextlost', handleContextLost);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      renderer?.dispose();
      resources.forEach(resource => resource.dispose());
    };
  }, [onStatus, particlesEnabled, intensity, visualization, prefersReducedMotion]);

  return <canvas ref={canvasRef} className={`three-planet-canvas${canvasReady ? ' ready' : ''}`} aria-hidden="true" />;
}

function addOrbit(THREE: typeof import('three'), group: import('three').Group, resources: Array<{ dispose: () => void }>, radius: number, color: import('three').Color, tilt: number, spin: number, opacity: number) {
  const geometry = new THREE.TorusGeometry(radius, .005, 4, 192);
  const material = new THREE.MeshBasicMaterial({ color, transparent: true, opacity });
  const ring = new THREE.Mesh(geometry, material);
  ring.rotation.x = tilt;
  ring.rotation.y = spin;
  group.add(ring);
  resources.push(geometry, material);
}

function addStarfield(THREE: typeof import('three'), scene: import('three').Scene, resources: Array<{ dispose: () => void }>, count: number, opacity: number) {
  let seed = 37;
  const random = () => { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; };
  const positions = new Float32Array(count * 3);
  for (let i=0;i<count;i++) { positions[i*3]=(random()-.5)*8; positions[i*3+1]=(random()-.5)*5; positions[i*3+2]=-1-random()*5; }
  const geometry=new THREE.BufferGeometry();geometry.setAttribute('position',new THREE.BufferAttribute(positions,3));
  const material=new THREE.PointsMaterial({color:'#cce8ef',size:.018,sizeAttenuation:true,transparent:true,opacity});
  scene.add(new THREE.Points(geometry,material));resources.push(geometry,material);
}

function addAtmosphere(THREE: typeof import('three'), scene: import('three').Scene, resources: Array<{ dispose: () => void }>, radius: number, color: import('three').Color) {
  const geometry = new THREE.SphereGeometry(radius, 64, 48);
  const material = new THREE.MeshBasicMaterial({ color, transparent: true, opacity: .13, side: THREE.BackSide });
  const atmosphere = new THREE.Mesh(geometry, material);
  scene.add(atmosphere);
  resources.push(geometry, material);
}
