#!/usr/bin/env python3
import time
import board
import neopixel
import numpy as np

N = 5*60-1
pixels1 = neopixel.NeoPixel(board.D18, N, brightness=0.1, auto_write=False)
pixels1.fill((0, 100, 0))
max_mu, sigma = 128, 16
try:
    while True:
        for i in range(N):
            mu = i * 128 / N
            n = [0,0,0]
            r = np.random.normal(mu, sigma, 1)
            b = np.random.normal(mu / 10, sigma, 1)
            r = int(np.rint(np.clip(r, 0, 255)).astype(int))
            b = int(np.rint(np.clip(b, 0, 255)).astype(int))
            n[0] = r
            n[2] = b
            n = tuple(n)
            pixels1[i] = n
        pixels1.show()
finally:
    pixels1.fill((0, 0, 0))
