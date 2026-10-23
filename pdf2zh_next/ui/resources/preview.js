() => {
    'use strict';

function adjustPDFCanvasScale() {
        var previewContainers = document.querySelectorAll('.pdf-preview-fixed');
        for (var i = 0; i < previewContainers.length; i++) {
            var container = previewContainers[i];
            var canvas = container.querySelector('canvas');
        if (!canvas) {
                continue;
        }

            var containerRect = container.getBoundingClientRect();
            var containerHeight = containerRect.height - 24;
            var containerWidth = containerRect.width - 24;

            var canvasWidth = canvas.naturalWidth || canvas.width || canvas.offsetWidth;
            var canvasHeight = canvas.naturalHeight || canvas.height || canvas.offsetHeight;

            if (!canvasWidth || !canvasHeight) {
                continue;
            }

            var scaleHeight = containerHeight / canvasHeight;
            var scaleWidth = containerWidth / canvasWidth;
            var scale = Math.min(scaleHeight, scaleWidth);

            var scaledWidth = canvasWidth * scale;
            var scaledHeight = canvasHeight * scale;

        canvas.style.width = scaledWidth + 'px';
        canvas.style.height = scaledHeight + 'px';
        canvas.style.maxWidth = '100%';
        canvas.style.maxHeight = '100%';

        // 深色模式下：为 canvas 添加包装器，使背景大小精确匹配 PDF 内容
        var bodyStyle = window.getComputedStyle(document.body);
        var bgColor = bodyStyle.backgroundColor;
        var isDarkMode = document.body.classList.contains('dark') ||
                         document.documentElement.getAttribute('data-theme') === 'dark' ||
                         document.documentElement.classList.contains('dark') ||
                         (bgColor && (bgColor.indexOf('rgb(30') === 0 ||
                                      bgColor.indexOf('rgb(31') === 0 ||
                                      bgColor.indexOf('rgb(32') === 0 ||
                                      bgColor.indexOf('#1') === 0 ||
                                      bgColor.indexOf('#2') === 0));

        if (isDarkMode) {
            // 检查是否已有包装器
            var wrapper = canvas.parentElement;
            var needsWrapper = !wrapper.classList || !wrapper.classList.contains('pdf-canvas-wrapper');

            if (needsWrapper && wrapper !== container) {
                // 创建包装器
                var newWrapper = document.createElement('div');
                newWrapper.className = 'pdf-canvas-wrapper';
                wrapper.insertBefore(newWrapper, canvas);
                newWrapper.appendChild(canvas);
                wrapper = newWrapper;
            } else if (wrapper === container || !wrapper.classList.contains('pdf-canvas-wrapper')) {
                // 如果 canvas 直接是 container 的子元素，创建包装器
                var newWrapper = document.createElement('div');
                newWrapper.className = 'pdf-canvas-wrapper';
                container.insertBefore(newWrapper, canvas);
                newWrapper.appendChild(canvas);
                wrapper = newWrapper;
            }

            // 设置包装器大小，稍微大一点以包含 padding 和阴影
            if (wrapper && wrapper.classList.contains('pdf-canvas-wrapper')) {
                wrapper.style.width = (scaledWidth + 16) + 'px';  // 8px padding * 2
                wrapper.style.height = (scaledHeight + 16) + 'px';
                wrapper.style.maxWidth = '100%';
                wrapper.style.maxHeight = '100%';
                wrapper.style.margin = '0 auto';
                wrapper.style.display = 'inline-block';
            }

            // 确保 canvas 本身没有额外的背景
            canvas.style.background = 'transparent';
        } else {
            // 浅色模式下移除包装器样式
            canvas.style.background = '#ffffff';
        }
        }
    }

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', adjustPDFCanvasScale);
} else {
    adjustPDFCanvasScale();
}

    var observer = new MutationObserver(adjustPDFCanvasScale);
observer.observe(document.body, { childList: true, subtree: true });

window.addEventListener('resize', adjustPDFCanvasScale);
}
