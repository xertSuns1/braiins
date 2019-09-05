include $(TOPDIR)/rules.mk

PKG_NAME:=bosminer_monitor
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/bosminer_monitor
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Monitor for bOSminer
  DEPENDS:=+bosminer
endef

define Package/bosminer_monitor/description
 Monitors if bOSminer is working properly and provide signalization about its
 current state.
endef

define Build/Compile
endef

define Package/bosminer_monitor/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/bosminer_monitor.sh $(1)/usr/sbin/

	$(INSTALL_DIR) $(1)/etc/init.d/
	$(INSTALL_BIN) ./files/bosminer_monitor.init $(1)/etc/init.d/bosminer_monitor
endef

$(eval $(call BuildPackage,bosminer_monitor))
