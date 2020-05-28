all: $(PDF)

DL_FOLDER=download
TOOLCHAIN_FOLDER=toolchain
RAMFS_ROOT=ramfs_root
BUILD_ENV_FOLDER=build_env
CROSS_COMPILE_PREFIX=$(TOOLCHAIN_FOLDER)/arm-unknown-linux-gnueabihf/bin/armv6l-unknown-linux-gnueabihf-
CC=$(CROSS_COMPILE_PREFIX)gcc
PACKAGE_ARCHIVES=$(DL_FOLDER)/alsa.tar.xz $(DL_FOLDER)/glibc.tar.xz $(DL_FOLDER)/gcclibs.tar.xz
BUILD_LIBC=$(BUILD_ENV_FOLDER)/usr/lib/libc.so
RAMFS_LIBS=$(RAMFS_ROOT)/usr/lib/libc.so $(RAMFS_ROOT)/usr/lib/libasound.so $(RAMFS_ROOT)/usr/lib/libgcc_s.so
INITRAMFS=initramfs-linux.img
KERNEL_DIR=linux
KERNEL_REPO=https://github.com/raspberrypi/linux
KERNEL_BRANCH=rpi-4.19.y
KERNEL=vmlinuz-linux
NPROC=$(shell nproc)
RPI_CONFIG=config.txt

#CARGO_FLAGS=
#INIT=target/arm-unknown-linux-gnueabihf/debug/kassette
CARGO_FLAGS=--release
INIT=target/arm-unknown-linux-gnueabihf/release/kassette

SCP_TARGET=alarm@10.0.0.13:
BOOT_MOUNT=/media/sdf1-usb-Generic_STORAGE_/

all: initramfs kernel

initramfs: $(INITRAMFS)

kernel: $(KERNEL)

scp: $(INIT)
	scp $< $(SCP_TARGET)

sd: $(INITRAMFS) $(KERNEL) $(RPI_CONFIG)
	cp $^ $(BOOT_MOUNT)

$(DL_FOLDER)/alsa.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "http://mirror.archlinuxarm.org/armv6h/extra/alsa-lib-1.2.2-1-armv6h.pkg.tar.xz" -O $@

$(DL_FOLDER)/glibc.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "http://mirror.archlinuxarm.org/armv6h/core/glibc-2.31-2-armv6h.pkg.tar.xz" -O $@

$(DL_FOLDER)/gcclibs.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "http://mirror.archlinuxarm.org/armv6h/core/gcc-libs-9.3.0-1-armv6h.pkg.tar.xz" -O $@

$(DL_FOLDER)/x-tools.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "https://archlinuxarm.org/builder/xtools/x-tools6h.tar.xz" -O $@

$(CC): $(DL_FOLDER)/x-tools.tar.xz
	tar --touch -xf $^
	chmod -R 777 $(TOOLCHAIN_FOLDER) || true
	rm -rf $(TOOLCHAIN_FOLDER)
	mv x-tools6h $(TOOLCHAIN_FOLDER)

$(BUILD_LIBC): $(PACKAGE_ARCHIVES)
	mkdir -p $(BUILD_ENV_FOLDER)
	for archive in $^; do tar -C $(BUILD_ENV_FOLDER) -xf $$archive; done
	sed -i "s#/usr/#$(BUILD_ENV_FOLDER)/usr/#g" $(BUILD_LIBC)

$(INIT): $(BUILD_LIBC) $(CC) src/main.rs
	PKG_CONFIG_ALLOW_CROSS=1 cargo build $(CARGO_FLAGS)

$(RAMFS_ROOT)/lib:
	mkdir -p $(RAMFS_ROOT)
	if [ ! -L $@ ]; then ln -s /usr/lib $@; fi

$(RAMFS_ROOT)/dev:
	mkdir -p $@

$(RAMFS_ROOT)/proc:
	mkdir -p $@

$(RAMFS_ROOT)/init: $(INIT)
	cp $< $@

$(RAMFS_ROOT)/usr/lib/libc.so: $(DL_FOLDER)/glibc.tar.xz
	mkdir -p $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) --touch -xf $<
	rm -rf\
		$(RAMFS_ROOT)/usr/lib/gconv\
		$(RAMFS_ROOT)/usr/lib/*.a\
		$(RAMFS_ROOT)/usr/share/i18n\
		$(RAMFS_ROOT)/usr/share/locale\
		$(RAMFS_ROOT)/usr/share/info\
		$(RAMFS_ROOT)/usr/include\
		$(RAMFS_ROOT)/usr/bin\

$(RAMFS_ROOT)/usr/lib/libasound.so: $(DL_FOLDER)/alsa.tar.xz
	mkdir -p $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) --touch -xf $<
	rm -rf\
		$(RAMFS_ROOT)/usr/include\
		$(RAMFS_ROOT)/usr/bin\

$(RAMFS_ROOT)/usr/lib/libgcc_s.so: $(DL_FOLDER)/gcclibs.tar.xz
	mkdir -p $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) --touch -xf $<
	rm -rf\
		$(RAMFS_ROOT)/usr/lib/libgo.so.*\
		$(RAMFS_ROOT)/usr/lib/libgphobos.so.*\
		$(RAMFS_ROOT)/usr/lib/libstdc++.so.*\
		$(RAMFS_ROOT)/usr/lib/libasan.so.*\
		$(RAMFS_ROOT)/usr/lib/libgdruntime.so.*\
		$(RAMFS_ROOT)/usr/lib/libgfortran.so.*\
		$(RAMFS_ROOT)/usr/lib/libubsan.so.*\
		$(RAMFS_ROOT)/usr/lib/libgomp.so.*\
		$(RAMFS_ROOT)/usr/lib/libobjc.so.*\
		$(RAMFS_ROOT)/usr/share/locale\
		$(RAMFS_ROOT)/usr/share/info\

$(INITRAMFS): $(RAMFS_LIBS) $(RAMFS_ROOT)/lib $(RAMFS_ROOT)/proc $(RAMFS_ROOT)/dev $(RAMFS_ROOT)/init
	cd $(RAMFS_ROOT) && find | cpio -ov --format=newc | gzip -9 > ../$@

$(KERNEL_DIR)/Makefile:
	git clone --depth=1 --branch $(KERNEL_BRANCH) $(KERNEL_REPO) $(KERNEL_DIR)

$(KERNEL_DIR)/.config: kernel_config
	cp $< $@

$(KERNEL_DIR)/arch/arm/boot/zImage: $(KERNEL_DIR)/Makefile $(KERNEL_DIR)/.config $(CC)
	$(MAKE) -j $(NPROC) -C $(KERNEL_DIR) ARCH="arm" CROSS_COMPILE=$(shell pwd)/$(CROSS_COMPILE_PREFIX) zImage

$(KERNEL): $(KERNEL_DIR)/arch/arm/boot/zImage
	cp $< $@

.PHONY: clean all kernel initramfs scp sd

clean:
	chmod -R 777 $(RAMFS_ROOT) || true
	chmod -R 777 $(BUILD_ENV_FOLDER) || true
	chmod -R 777 $(TOOLCHAIN_FOLDER) || true
	rm -rf $(DL_FOLDER) $(TOOLCHAIN_FOLDER) $(RAMFS_ROOT) $(BUILD_ENV_FOLDER) $(INITRAMFS) $(KERNEL_DIR) $(KERNEL)
