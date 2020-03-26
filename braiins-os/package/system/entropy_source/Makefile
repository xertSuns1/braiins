# Copyright (C) 2020  Braiins Systems s.r.o.
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

PKG_NAME:=entropy_source
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

include $(INCLUDE_DIR)/package.mk

define Package/entropy_source
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=ADC entropy source
endef

define Package/entropy_source/description
 Gather some entropy from ADC and feed it to /dev/random to speed up
 entropy generation on boot time.
endef

define Build/Compile
	$(TARGET_CROSS)gcc $(TARGET_CFLAGS) $(TARGET_CPPFLAGS) $(TARGET_LDFLAGS) -o $(PKG_BUILD_DIR)/add-entropy ./files/add-entropy.c
endef

define Package/entropy_source/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) ./files/adc-noise.lua $(1)/usr/bin/adc-noise.lua
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/add-entropy $(1)/usr/bin/add-entropy

	$(INSTALL_DIR) $(1)/etc/init.d/
	$(INSTALL_BIN) ./files/entropy_source.init $(1)/etc/init.d/entropy_source
endef

$(eval $(call BuildPackage,entropy_source))
