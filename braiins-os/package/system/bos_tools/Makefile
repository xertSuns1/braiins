# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

include $(TOPDIR)/rules.mk

PKG_NAME:=bos_tools
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/bos_tools
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Utilities for managing braiins OS
  DEPENDS:=+socat
endef

define Package/bos_tools/description
 Add bos script which is used for firmware management.
 It can do factory reset, boot to recovery mode, ...
endef

define Build/Compile
endef

define Package/bos_tools/conffiles
/etc/config/bos
endef

define Package/bos_tools/install
	$(INSTALL_DIR) $(1)/etc/
	$(CP) ./files/factory_cfg.config $(1)/etc
	$(INSTALL_DIR) $(1)/etc/config
	$(INSTALL_CONF) ./files/bos.config $(1)/etc/config/bos
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/bos $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/run-background-process $(1)/usr/sbin/
endef

$(eval $(call BuildPackage,bos_tools))
