# R5 构建期效果纹理

这些 SVG 是本项目的效果素材，随官方 vendor 的 SVG 一起由 build-assets.mjs 确定性栅格化及打包。
`fx-nuke-blur.svg` 使用官方 nuke.svg 图形（ISC 许可见 ../svg/LICENSE.txt）并增加留白与构建期模糊；其余为程序几何。
官方原始资产目录保持原样。

- beam：12/18 世界单位的白束主体，blur=5 的 ADD 辅束，以及 lab 用 blur=2。256px 横向纹理拉伸，采样头尾窗口；运行中不创建 BlurFilter 或重绘 Graphics。
- nuke：四档预模糊圆（世界 blur 30/20/10/0.5），按固定 phase 混合；marker 另带 blur=2 的纹理。

与官方 GPU BlurFilter 的采样存在有界近似：beam 端部采用窗口裁切，缺少滤镜超出端点的柔边；nuke 连续 blur 由四档纹理插值替代。
纵向束边、时序、颜色和混合路径已人工对照双后端；这些近似仍登记 G7，不能宣称逐像素一致。
所有实例共享 atlas 纹理；实例销毁只销毁节点，纹理由 renderer 的资产租约统一释放。
