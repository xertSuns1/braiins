include $(TOPDIR)/rules.mk

PKG_NAME:=bosminer
PKG_VERSION:=1.0.0
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

ifeq ($(strip $(CONFIG_EXTERNAL_BOSMINER_TREE)),"")
	PKG_SOURCE_PROTO:=git
	PKG_SOURCE_URL:=git@gitlab.bo:x/bosminer.git
	PKG_SOURCE_VERSION:=master
else
	PKG_SOURCE_VERSION:=local
    USE_SOURCE_DIR:=$(call qstrip,$(CONFIG_EXTERNAL_BOSMINER_TREE))
endif

PKG_SOURCE_SUBDIR:=$(PKG_NAME)-$(PKG_SOURCE_VERSION)
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_SOURCE_SUBDIR)

CARGO_TARGET:=arm-unknown-linux-musleabi
CARGO_OUTPUT_DIR:=$(PKG_BUILD_DIR)/target/$(CARGO_TARGET)/release

BOSMINER_BACKEND:=\
	$(if $(CONFIG_BOSMINER_BACKEND_antminer_s9),antminer_s9,\
	$(if $(CONFIG_BOSMINER_BACKEND_erupter),erupter))

include $(INCLUDE_DIR)/package.mk

define Package/bosminer
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=bOSminer mining software with Stratum V2
  URL:=https://braiins-os.org/
endef

define Package/bosminer/description
 braiins mining software with Stratum V2 protocol.
endef

define Package/bosminer/config
	source "$(SOURCE)/Config.in"
endef

define Build/Compile
	( cd $(PKG_BUILD_DIR); CROSS_COMPILE=$(TARGET_CROSS) \
	  cargo build --release --features $(BOSMINER_BACKEND) --target $(CARGO_TARGET))
	$(TARGET_CROSS)strip $(CARGO_OUTPUT_DIR)/$(PKG_NAME)
endef

define Package/bosminer/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(CARGO_OUTPUT_DIR)/$(PKG_NAME) $(1)/usr/bin/
endef

$(eval $(call BuildPackage,bosminer))
