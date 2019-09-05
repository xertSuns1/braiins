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

PKG_NAME:=bos_recovery
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/bos_recovery
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Utilities for system recovery mode
endef

define Package/bos_recovery/description
 Do some action depending on the U-Boot environment variables.
 It is used e.g. for factory reset.
endef

define Build/Compile
endef

define Package/bos_recovery/install
	$(INSTALL_DIR) $(1)/etc/
	$(CP) ./files/bos_mode $(1)/etc/

	$(INSTALL_DIR) $(1)/etc/profile.d/
	$(INSTALL_BIN) ./files/bos_recovery.profile $(1)/etc/profile.d/bos_recovery.sh

	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/bos_recovery.sh $(1)/usr/sbin/

	$(INSTALL_DIR) $(1)/etc/init.d/
	$(INSTALL_BIN) ./files/bos_recovery.init $(1)/etc/init.d/bos_recovery
endef

$(eval $(call BuildPackage,bos_recovery))
