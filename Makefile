all: $(PDF)

DL_FOLDER=download
TOOLCHAIN_FOLDER=toolchain
RAMFS_ROOT=ramfs_root
BUILD_ENV_FOLDER=build_env
CC=$(TOOLCHAIN_FOLDER)/arm-unknown-linux-gnueabihf/bin/armv6l-unknown-linux-gnueabihf-gcc
PACKAGE_ARCHIVES=$(DL_FOLDER)/alsa.tar.xz $(DL_FOLDER)/glibc.tar.xz $(DL_FOLDER)/gcclibs.tar.xz
BUILD_LIBC="$(BUILD_ENV_FOLDER)/usr/lib/libc.so"
INIT="target/arm-unknown-linux-gnueabihf/debug/rfid_player"
RAMFS_LIBS=$(RAMFS_ROOT)/usr/lib/libc.so $(RAMFS_ROOT)/usr/lib/libasound.so $(RAMFS_ROOT)/usr/lib/libgcc_s.so
INITRAMFS=initramfs-linux.img

all: $(INITRAMFS)

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

toolchain: $(DL_FOLDER)/x-tools.tar.xz
	tar -xf $^
	mv x-tools6h $(TOOLCHAIN_FOLDER)

$(CC): toolchain

build_env: $(PACKAGE_ARCHIVES)
	mkdir -p $(BUILD_ENV_FOLDER)
	for archive in $^; do tar -C $(BUILD_ENV_FOLDER) -xf $$archive; done
	sed -i "s#/usr/#$(BUILD_ENV_FOLDER)/usr/#g" $(BUILD_LIBC)

$(INIT): build_env $(CC)
	PKG_CONFIG_ALLOW_CROSS=1 cargo build

$(RAMFS_ROOT):
	mkdir -p $(RAMFS_ROOT)

$(RAMFS_ROOT)/lib: $(RAMFS_ROOT)
	if [ ! -L $@ ]; then ln -s /usr/lib $@; fi

$(RAMFS_ROOT)/dev: $(RAMFS_ROOT)
	mkdir -p $@

$(RAMFS_ROOT)/proc: $(RAMFS_ROOT)
	mkdir -p $@

$(RAMFS_ROOT)/init: $(INIT)
	cp $< $@

$(RAMFS_ROOT)/usr/lib/libc.so: $(DL_FOLDER)/glibc.tar.xz $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) -xf $<
$(RAMFS_ROOT)/usr/lib/libasound.so: $(DL_FOLDER)/alsa.tar.xz $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) -xf $<
$(RAMFS_ROOT)/usr/lib/glibc_s.so: $(DL_FOLDER)/gcclibs.tar.xz $(RAMFS_ROOT)
	tar -C $(RAMFS_ROOT) -xf $<

ramfs_libs: $(RAMFS_LIBS)
	# Remove unused libraries
	rm -rf\
		$(RAMFS_ROOT)/usr/lib/libgo.so.*\
		$(RAMFS_ROOT)/usr/lib/libgphobos.so.*\
		$(RAMFS_ROOT)/usr/lib/libstdc++.so.*\
		$(RAMFS_ROOT)/usr/lib/libasan.so.*\
		$(RAMFS_ROOT)/usr/lib/gconv\
		$(RAMFS_ROOT)/usr/lib/libgdruntime.so.*\
		$(RAMFS_ROOT)/usr/lib/libgfortran.so.*\
		$(RAMFS_ROOT)/usr/lib/libubsan.so.*\
		$(RAMFS_ROOT)/usr/lib/libgomp.so.*\
		$(RAMFS_ROOT)/usr/lib/libobjc.so.*\
		$(RAMFS_ROOT)/usr/lib/*.a\
		$(RAMFS_ROOT)/usr/share/i18n\
		$(RAMFS_ROOT)/usr/share/locale\
		$(RAMFS_ROOT)/usr/share/info\
		$(RAMFS_ROOT)/usr/include\
		$(RAMFS_ROOT)/usr/bin\

$(INITRAMFS): ramfs_libs $(RAMFS_ROOT)/lib $(RAMFS_ROOT)/proc $(RAMFS_ROOT)/dev $(RAMFS_ROOT)/init
	cd $(RAMFS_ROOT) && find | cpio -ov --format=newc | gzip -9 > $@

.PHONY: clean

clean:
	rm -rf $(DL_FOLDER) $(TOOLCHAIN_FOLDER) $(RAMFS_ROOT) $(BUILD_ENV_FOLDER) $(INITRAMFS)
