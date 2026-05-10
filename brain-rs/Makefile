# Build commands for Shadow Warrior Brain
IMAGE_NAME = brain-build

.PHONY: help image build-pi4 build-pi0 clean

help:
	@echo "Available commands:"
	@echo "  make image       - Build the podman build image"
	@echo "  make build-pi4   - Build for Raspberry Pi 4 (AArch64)"
	@echo "  make build-pi0   - Build for Raspberry Pi Zero W (ARMhf)"
	@echo "  make clean       - Remove build artifacts"

image:
	podman build -t $(IMAGE_NAME) -f Dockerfile.build .

build-pi4:
	podman run --rm -v "$$(pwd)":/project -w /project $(IMAGE_NAME) \
		cargo build --target aarch64-unknown-linux-gnu

build-pi0:
	podman run --rm -v "$$(pwd)":/project -w /project \
		-e PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig \
		$(IMAGE_NAME) \
		cargo build --target arm-unknown-linux-gnueabihf

clean:
	podman run --rm -v "$$(pwd)":/project -w /project $(IMAGE_NAME) cargo clean
