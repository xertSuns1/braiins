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
	$(INSTALL_BIN) ./files/set_fan.sh $(1)/usr/bin/set_fan
endef

$(eval $(call BuildPackage,bosminer))
