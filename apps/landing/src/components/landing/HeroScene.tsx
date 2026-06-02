import type { RefObject } from "react";
import { AppConstellation } from "./AppConstellation";
import { FloatingHeroCard } from "./FloatingHeroCard";
import { ProductMockup } from "./ProductMockup";
import { RiveMemoryEngine } from "./RiveMemoryEngine";
import { assets } from "./landingData";

type HeroSceneProps = {
  sceneRef: RefObject<HTMLDivElement>;
};

export function HeroScene({ sceneRef }: HeroSceneProps) {
  return (
    <div className="hero-scene" ref={sceneRef} aria-label="Animated DayTrail product preview">
      <p className="engine-label" aria-hidden="true">
        Work memory
      </p>

      <AppConstellation />
      <RiveMemoryEngine />

      <div className="mockup-layer">
        <ProductMockup />
      </div>

      <FloatingHeroCard
        body="VS Code · DayTrail repo"
        className="card-capture"
        detail="resume point saved"
        icon={assets.apps.vscode}
        title="Context saved"
      />
      <FloatingHeroCard
        body="Prompts and tools linked"
        className="card-ai"
        icon={assets.apps.chatgpt}
        title="AI context logged"
      />
      <FloatingHeroCard
        body="Weekly notes drafted"
        className="card-report"
        title="Update ready"
        variant="report"
      />
    </div>
  );
}
