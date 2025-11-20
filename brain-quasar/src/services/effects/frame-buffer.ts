// Frame Buffer Utility for LED Pixel Manipulation

export class FrameBuffer {
  public pixels: Uint8Array;
  public readonly ledCount: number;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.pixels = new Uint8Array(ledCount * 3); // RGB for each LED
  }

  /**
   * Set color for a specific LED
   */
  setPixel(index: number, r: number, g: number, b: number): void {
    if (index < 0 || index >= this.ledCount) return;

    const offset = index * 3;
    this.pixels[offset] = Math.max(0, Math.min(255, Math.floor(r)));
    this.pixels[offset + 1] = Math.max(0, Math.min(255, Math.floor(g)));
    this.pixels[offset + 2] = Math.max(0, Math.min(255, Math.floor(b)));
  }

  /**
   * Get RGB color for a specific LED
   */
  getPixel(index: number): [number, number, number] {
    if (index < 0 || index >= this.ledCount) return [0, 0, 0];

    const offset = index * 3;
    return [
      this.pixels[offset]!,
      this.pixels[offset + 1]!,
      this.pixels[offset + 2]!,
    ];
  }

  /**
   * Fill all LEDs with a single color
   */
  fill(r: number, g: number, b: number): void {
    for (let i = 0; i < this.ledCount; i++) {
      this.setPixel(i, r, g, b);
    }
  }

  /**
   * Clear all LEDs (set to black/off)
   */
  clear(): void {
    this.pixels.fill(0);
  }

  /**
   * Copy pixels from another buffer
   */
  copyFrom(other: FrameBuffer): void {
    if (other.ledCount !== this.ledCount) {
      throw new Error('Buffer size mismatch');
    }
    this.pixels.set(other.pixels);
  }

  /**
   * Blend another buffer on top using alpha blending
   */
  blendFrom(other: FrameBuffer, alpha: number): void {
    if (other.ledCount !== this.ledCount) {
      throw new Error('Buffer size mismatch');
    }

    const invAlpha = 1 - alpha;
    for (let i = 0; i < this.pixels.length; i++) {
      this.pixels[i] = Math.floor(
        this.pixels[i]! * invAlpha + other.pixels[i]! * alpha
      );
    }
  }

  /**
   * Get a copy of the pixel data
   */
  clone(): FrameBuffer {
    const buffer = new FrameBuffer(this.ledCount);
    buffer.copyFrom(this);
    return buffer;
  }

  /**
   * Get pixel data in wire format (GRB if isGrb=true, RGB otherwise)
   */
  getWireData(): Uint8Array {
    return this.pixels.slice(); // RGB order
  }

  /**
   * Apply gradient between two colors
   */
  gradient(
    startR: number, startG: number, startB: number,
    endR: number, endG: number, endB: number,
    startIndex = 0,
    endIndex = this.ledCount - 1
  ): void {
    const length = endIndex - startIndex + 1;
    if (length <= 0) return;

    for (let i = 0; i < length; i++) {
      const t = i / (length - 1);
      const r = startR + (endR - startR) * t;
      const g = startG + (endG - startG) * t;
      const b = startB + (endB - startB) * t;
      this.setPixel(startIndex + i, r, g, b);
    }
  }
}
