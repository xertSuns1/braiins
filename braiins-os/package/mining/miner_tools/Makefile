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
include $(INCLUDE_DIR)/kernel.mk

PKG_NAME:=miner_tools
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/miner_tools
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Utilities for managing miner
  DEPENDS:=+bos_tools
endef

define Package/miner_tools/description
 Add miner script which is used for miner namagement.
 It can do factory reset, boot to recovery mode or turn on miner's LEDs.
endef

define Build/Compile
endef

ifeq ($(SUBTARGET),am1-s9)
define Package/miner_tools/subtarget_install
	$(CP) ./files/CONFIG.am $(1)/usr/share/upgrade/CONFIG
endef
endif

ifneq (,$(filter dm1-g9 dm1-g19 dm1-g29,$(SUBTARGET)))
define Package/miner_tools/subtarget_install
	$(CP) ./files/CONFIG.dm $(1)/usr/share/upgrade/CONFIG
endef
endif

define Package/miner_tools/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/miner $(1)/usr/sbin/
	$(INSTALL_DIR) $(1)/usr/share/upgrade
	$(CP) ./files/CONTROL $(1)/usr/share/upgrade/CONTROL.bos
	$(call Package/miner_tools/subtarget_install,$(1))
endef

$(eval $(call BuildPackage,miner_tools))
