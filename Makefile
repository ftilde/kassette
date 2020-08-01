all: $(PDF)

DL_FOLDER=download
TOOLCHAIN_FOLDER=toolchain
TOOLCHAIN_FOLDER_MUSL=toolchain_musl
RAMFS_ROOT=ramfs_root
BUILD_ENV_FOLDER=build_env
CROSS_COMPILE_PREFIX=$(TOOLCHAIN_FOLDER)/arm-unknown-linux-gnueabihf/bin/armv6l-unknown-linux-gnueabihf-
CC=$(CROSS_COMPILE_PREFIX)gcc
CROSS_COMPILE_PREFIX_MUSL=$(TOOLCHAIN_FOLDER_MUSL)/bin/arm-linux-musleabihf-
CC_MUSL=$(CROSS_COMPILE_PREFIX_MUSL)gcc
PACKAGE_ARCHIVES=$(DL_FOLDER)/alsa.tar.xz $(DL_FOLDER)/glibc.tar.xz $(DL_FOLDER)/gcclibs.tar.xz
BUILD_LIBC=$(BUILD_ENV_FOLDER)/usr/lib/libc.so
INITRAMFS=initramfs-linux.img
KERNEL_DIR=linux
KERNEL_REPO=https://github.com/raspberrypi/linux
KERNEL_BRANCH=rpi-4.19.y
KERNEL=vmlinuz-linux
NPROC=$(shell nproc)
RPI_CONFIG=config.txt

#BUILD_TYPE=debug
BUILD_TYPE=release
TARGET=arm-unknown-linux-musleabihf
CARGO_FLAGS=--$(BUILD_TYPE) --target $(TARGET)
INIT=target/arm-unknown-linux-musleabihf/$(BUILD_TYPE)/kassette

SCP_TARGET=alarm@10.0.0.11:
BOOT_MOUNT=/media/sdf1-usb-Generic_STORAGE_/

ALSA=alsa-lib-1.2.3

all: initramfs kernel

initramfs: $(INITRAMFS)

kernel: $(KERNEL)

scp: $(INIT)
	scp $< $(SCP_TARGET)

sd: $(INITRAMFS) $(KERNEL) $(RPI_CONFIG)
	cp $^ $(BOOT_MOUNT)

$(DL_FOLDER)/$(ALSA).tar.bz2:
	mkdir -p $(DL_FOLDER)
	wget "ftp://ftp.alsa-project.org/pub/lib/$(ALSA).tar.bz2" -O $@

$(DL_FOLDER)/musl-tools.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "https://musl.cc/arm-linux-musleabihf-cross.tgz" -O $@

$(DL_FOLDER)/x-tools.tar.xz:
	mkdir -p $(DL_FOLDER)
	wget "https://archlinuxarm.org/builder/xtools/x-tools6h.tar.xz" -O $@

$(ALSA): $(DL_FOLDER)/$(ALSA).tar.bz2
	tar -xf $<

$(BUILD_ENV_FOLDER)/usr/lib/libasound.a: $(ALSA) $(CC_MUSL)
	cd $(ALSA) && CFLAGS="-mtune=arm1176jzf-s" ./configure --enable-static --disable-shared CC="$(shell pwd)/$(CC_MUSL)" --host=arm-linux-musl --without-debug
	cd $(ALSA) && $(MAKE) -j $(NPROC)
	mkdir -p $(BUILD_ENV_FOLDER)/usr/lib
	cp $(ALSA)/src/.libs/* $(BUILD_ENV_FOLDER)/usr/lib/

$(RAMFS_ROOT)/usr/share/alsa: $(ALSA)
	mkdir -p $(RAMFS_ROOT)/usr/share
	cp -r $(ALSA)/src/conf $@
	find ramfs_root -name "Makefile*" -exec rm {} \;

$(BUILD_ENV_FOLDER)/usr/lib/libc.a: $(CC_MUSL)
	mkdir -p $(BUILD_ENV_FOLDER)/usr/lib
	cp $(TOOLCHAIN_FOLDER_MUSL)/arm-linux-musleabihf/lib/libc.a $@
	#So... If we don't do the following, we get some "multiple definition"
	# errors when linking. It seems like gcc thinks that they belong into libc
	# and llvm (and thus rust) thinks they should be in compiler-rt, hence they
	# appear twice. As we only need one definition of those (assuming they are
	# compatible!), we just make the definition in the libc inaccessible by
	# renaming them.
	sed -i "s/__aeabi_memset/fooeabi_memset/g" $@
	sed -i "s/__aeabi_memmove/fooeabi_memmove/g" $@
	sed -i "s/__aeabi_memclr/fooeabi_memclr/g" $@
	sed -i "s/__aeabi_memcpy/fooeabi_memcpy/g" $@

$(CC_MUSL): $(DL_FOLDER)/musl-tools.tar.xz
	@echo $^
	tar --touch -xf $^
	chmod -R 777 $(TOOLCHAIN_FOLDER_MUSL) || true
	rm -rf $(TOOLCHAIN_FOLDER_MUSL)
	mv arm-linux-musleabihf-cross $(TOOLCHAIN_FOLDER_MUSL)

$(CC): $(DL_FOLDER)/x-tools.tar.xz
	tar --touch -xf $^
	chmod -R 777 $(TOOLCHAIN_FOLDER) || true
	rm -rf $(TOOLCHAIN_FOLDER)
	mv x-tools6h $(TOOLCHAIN_FOLDER)

$(BUILD_LIBC): $(PACKAGE_ARCHIVES)
	mkdir -p $(BUILD_ENV_FOLDER)
	for archive in $^; do tar -C $(BUILD_ENV_FOLDER) -xf $$archive; done
	sed -i "s#/usr/#$(BUILD_ENV_FOLDER)/usr/#g" $(BUILD_LIBC)

$(INIT): $(BUILD_ENV_FOLDER)/usr/lib/libasound.a $(BUILD_ENV_FOLDER)/usr/lib/libc.a $(CC_MUSL) src/*.rs
	PKG_CONFIG_ALLOW_CROSS=1 cargo build $(CARGO_FLAGS)
	$(CROSS_COMPILE_PREFIX_MUSL)strip $(INIT)

$(RAMFS_ROOT)/dev:
	mkdir -p $@

$(RAMFS_ROOT)/proc:
	mkdir -p $@

$(RAMFS_ROOT)/data:
	mkdir -p $@

$(RAMFS_ROOT)/init: $(INIT)
	cp $< $@

$(INITRAMFS): $(RAMFS_ROOT)/proc $(RAMFS_ROOT)/dev $(RAMFS_ROOT)/init $(RAMFS_ROOT)/data $(RAMFS_ROOT)/usr/share/alsa
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
	rm -rf $(DL_FOLDER) $(TOOLCHAIN_FOLDER) $(RAMFS_ROOT) $(BUILD_ENV_FOLDER) $(INITRAMFS) $(KERNEL_DIR) $(KERNEL) $(ALSA)
