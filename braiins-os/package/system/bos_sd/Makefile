include $(TOPDIR)/rules.mk

PKG_NAME:=bos_sd
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/bos_sd
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Utilities for system SD mode
endef

define Build/Compile
endef

define Package/bos_sd/install
	$(INSTALL_DIR) $(1)/etc/
	$(CP) ./files/bos_mode $(1)/etc/
endef

$(eval $(call BuildPackage,bos_sd))
