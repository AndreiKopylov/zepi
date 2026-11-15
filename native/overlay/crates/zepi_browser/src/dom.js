// Vendored from BB-fat/browser-use-rs (MIT), src/dom/extract_dom.js.
// ARIA Snapshot DOM Extraction
// Based on Playwright's ariaSnapshot.ts - generates ARIA-tree structure for AI consumption
(function() {
    'use strict';
    
    let currentIndex = 0;

    // Helper: normalize whitespace
    function normalizeWhiteSpace(text) {
        return text.replace(/\s+/g, ' ').trim();
    }

    // Helper: check if element is visible for ARIA
    function isElementHiddenForAria(element) {
        const tagName = element.tagName;
        if (['STYLE', 'SCRIPT', 'NOSCRIPT', 'TEMPLATE'].includes(tagName)) {
            return true;
        }
        
        const style = window.getComputedStyle(element);
        
        // Check display: contents
        if (style.display === 'contents' && element.nodeName !== 'SLOT') {
            let hasVisibleChild = false;
            for (let child of element.childNodes) {
                if (child.nodeType === 1 && !isElementHiddenForAria(child)) {
                    hasVisibleChild = true;
                    break;
                }
                if (child.nodeType === 3 && child.textContent && child.textContent.trim()) {
                    hasVisibleChild = true;
                    break;
                }
            }
            if (!hasVisibleChild) return true;
        }
        
        // Check visibility
        if (style.visibility !== 'visible') {
            return true;
        }
        
        // Check display: none and aria-hidden
        if (style.display === 'none') {
            return true;
        }
        
        if (element.getAttribute('aria-hidden') === 'true') {
            return true;
        }
        
        return false;
    }

    // Helper: check if element is visible (bounding box check)
    function isElementVisible(element) {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
    }

    // Helper: compute element box information
    function computeBox(element) {
        const style = window.getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        const visible = rect.width > 0 && rect.height > 0;
        const inline = style.display === 'inline';
        const cursor = style.cursor;
        
        return { visible, inline, cursor, rect };
    }

    // Helper: check if element receives pointer events
    function receivesPointerEvents(element) {
        const box = computeBox(element);
        if (!box.visible) return false;
        
        const style = window.getComputedStyle(element);
        return style.pointerEvents !== 'none';
    }

    // Helper: get ARIA role for element
    function getAriaRole(element) {
        // Check explicit role
        const explicitRole = element.getAttribute('role');
        if (explicitRole) {
            const roles = explicitRole.split(' ').map(r => r.trim());
            const validRole = roles[0]; // take first role
            if (validRole) return validRole;
        }
        
        if (element.isContentEditable) return 'textbox';

        // Implicit roles based on tag name
        const tagName = element.tagName;
        const implicitRoles = {
            'BUTTON': 'button',
            'A': element.hasAttribute('href') ? 'link' : null,
            'INPUT': getInputRole(element),
            'TEXTAREA': 'textbox',
            'SELECT': element.hasAttribute('multiple') || element.size > 1 ? 'listbox' : 'combobox',
            'H1': 'heading', 'H2': 'heading', 'H3': 'heading',
            'H4': 'heading', 'H5': 'heading', 'H6': 'heading',
            'IMG': element.getAttribute('alt') === '' ? 'presentation' : 'img',
            'NAV': 'navigation',
            'MAIN': 'main',
            'ARTICLE': 'article',
            'SECTION': element.hasAttribute('aria-label') || element.hasAttribute('aria-labelledby') ? 'region' : null,
            'HEADER': 'banner',
            'FOOTER': 'contentinfo',
            'ASIDE': 'complementary',
            'FORM': 'form',
            'TABLE': 'table',
            'UL': 'list', 'OL': 'list',
            'LI': 'listitem',
            'P': 'paragraph',
            'DIALOG': 'dialog',
            'IFRAME': 'iframe'
        };
        
        return implicitRoles[tagName] || 'generic';
    }

    function getInputRole(input) {
        const type = (input.type || 'text').toLowerCase();
        const roles = {
            'button': 'button',
            'checkbox': 'checkbox',
            'radio': 'radio',
            'range': 'slider',
            'search': 'searchbox',
            'text': 'textbox',
            'email': 'textbox',
            'tel': 'textbox',
            'url': 'textbox',
            'number': 'spinbutton'
        };
        return roles[type] || 'textbox';
    }

    // Helper: get accessible name for element
    function getElementAccessibleName(element) {
        // Check aria-label
        const ariaLabel = element.getAttribute('aria-label');
        if (ariaLabel) return ariaLabel;
        
        // Check aria-labelledby (may sit on an ancestor, e.g. the Quasar field root).
        let labelledBy = element.getAttribute('aria-labelledby');
        if (!labelledBy) {
            // Only fields inherit an ancestor's label; options/items keep their own text.
            const isField = ['INPUT','TEXTAREA','SELECT'].includes(element.tagName) ||
                ['combobox','textbox','searchbox','spinbutton'].includes(element.getAttribute('role'));
            if (isField) { const anc = element.closest('[aria-labelledby]'); if (anc) labelledBy = anc.getAttribute('aria-labelledby'); }
        }
        if (labelledBy) {
            const ids = labelledBy.split(/\s+/);
            const texts = ids.map(id => {
                const el = document.getElementById(id);
                return el ? el.textContent : '';
            }).filter(t => t);
            if (texts.length) return texts.join(' ');
        }
        
        // Quasar fields: the visible field label is the accessible name (placeholder is generic).
        const qf = element.closest && element.closest('.q-field');
        if (qf) { const ql = qf.querySelector('.q-field__label'); const t = ql && ql.textContent.trim(); if (t) return t; }
        
        // Check associated label (for inputs)
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA' || element.tagName === 'SELECT') {
            const id = element.id;
            if (id) {
                const label = document.querySelector('label[for="' + id + '"]');
                if (label) return label.textContent || '';
            }
            // Check if wrapped in label
            const parentLabel = element.closest('label');
            if (parentLabel) {
                return parentLabel.textContent || '';
            }
        }
        
        // Check alt for images
        if (element.tagName === 'IMG') {
            return element.getAttribute('alt') || '';
        }
        
        // Check title
        const title = element.getAttribute('title');
        if (title) return title;
        
        // Check placeholder for inputs
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
            const placeholder = element.getAttribute('placeholder');
            if (placeholder) return placeholder;
        }
        
        // Name-from-content roles (tabs/menuitems/options are often divs, not <button>).
        const roleAttr = element.getAttribute('role');
        if (['tab','menuitem','menuitemcheckbox','menuitemradio','option','switch','treeitem','button','link'].includes(roleAttr) ||
            element.tagName === 'A' || element.tagName === 'BUTTON') {
            const text = (element.textContent || '').trim();
            if (text) return text;
        }
        
        return '';
    }

    // Helper: get ARIA checked state
    function getAriaChecked(element) {
        const checked = element.getAttribute('aria-checked');
        if (checked === 'true') return true;
        if (checked === 'false') return false;
        if (checked === 'mixed') return 'mixed';
        
        // Native checkbox/radio
        if (element.tagName === 'INPUT') {
            if (element.type === 'checkbox' || element.type === 'radio') {
                return element.checked;
            }
        }
        
        return undefined;
    }

    // Helper: get ARIA disabled state
    function getAriaDisabled(element) {
        const disabled = element.getAttribute('aria-disabled');
        if (disabled === 'true') return true;
        
        // Native disabled
        if (element.disabled !== undefined) {
            return element.disabled;
        }
        
        return undefined;
    }

    // Helper: get ARIA expanded state
    function getAriaExpanded(element) {
        const expanded = element.getAttribute('aria-expanded');
        if (expanded === 'true') return true;
        if (expanded === 'false') return false;
        return undefined;
    }

    // Helper: get ARIA level
    function getAriaLevel(element) {
        const level = element.getAttribute('aria-level');
        if (level) {
            const num = parseInt(level, 10);
            if (!isNaN(num)) return num;
        }
        
        // Heading level
        if (element.tagName.match(/^H[1-6]$/)) {
            return parseInt(element.tagName[1], 10);
        }
        
        return undefined;
    }

    // Helper: get ARIA pressed state
    function getAriaPressed(element) {
        const pressed = element.getAttribute('aria-pressed');
        if (pressed === 'true') return true;
        if (pressed === 'false') return false;
        if (pressed === 'mixed') return 'mixed';
        return undefined;
    }

    // Helper: get ARIA selected state
    function getAriaSelected(element) {
        const selected = element.getAttribute('aria-selected');
        if (selected === 'true') return true;
        if (selected === 'false') return false;
        return undefined;
    }

    // Helper: get CSS content (::before, ::after)
    function getCSSContent(element, pseudo) {
        try {
            const style = window.getComputedStyle(element, pseudo);
            const content = style.content;
            if (content && content !== 'none' && content !== 'normal') {
                // Simple extraction - remove quotes
                return content.replace(/^["']|["']$/g, '');
            }
        } catch (e) {
            // Ignore errors
        }
        return '';
    }

    // Compute ARIA index for element
    function computeAriaIndex(ariaNode) {
        // Only assign indices to visible, interactive ARIA roles
        if (!ariaNode.box.visible) {
            return;
        }
        
        // Check if this is an interactive role that should have an index
        const interactiveRoles = [
            'button', 'link', 'textbox', 'searchbox', 'checkbox', 'radio',
            'combobox', 'listbox', 'option', 'menuitem', 'menuitemcheckbox',
            'menuitemradio', 'tab', 'tabpanel', 'slider', 'spinbutton',
            'switch', 'img', 'article', 'region', 'navigation', 'main',
            'complementary', 'banner', 'contentinfo', 'form', 'search',
            'tree', 'treeitem', 'grid', 'gridcell', 'row', 'columnheader',
            'rowheader', 'heading', 'dialog', 'alertdialog', 'alert',
            'status', 'progressbar', 'list', 'listitem', 'generic'
        ];
        
        // Only assign index to interactive roles, pointer cursor, or focusable (tabindex) widgets
        const hasPointerCursor = ariaNode.box.cursor === 'pointer';
        const isInteractiveRole = interactiveRoles.includes(ariaNode.role);
        const el = ariaNode.element;
        const focusable = el && el.hasAttribute && el.hasAttribute('tabindex') && el.tabIndex >= 0;
        
        if (!isInteractiveRole && !hasPointerCursor && !focusable) {
            return;
        }
        
        // Stable per-element id (survives re-renders; positional indices shift and go stale).
        const cache = window.__zepiBrowser ||= {ids:new WeakMap(), nodes:new Map(), next:1};
        cache.domIds ||= new WeakMap(); cache.domNext ||= 1;
        if (!cache.domIds.has(el)) cache.domIds.set(el, cache.domNext++);
        ariaNode.index = cache.domIds.get(el);
    }

    // Convert element to AriaNode
    function toAriaNode(element) {
        const active = document.activeElement === element;
        
        // Handle iframe specially
        if (element.tagName === 'IFRAME') {
            const ariaNode = {
                role: 'iframe',
                name: '',
                children: [],
                props: {},
                element: element,
                box: computeBox(element),
                receivesPointerEvents: true,
                active: active
            };
            computeAriaIndex(ariaNode);
            return ariaNode;
        }
        
        const role = getAriaRole(element);
        
        // Skip elements without role or with presentation/none
        if (!role || role === 'presentation' || role === 'none') {
            return null;
        }
        
        const name = normalizeWhiteSpace(getElementAccessibleName(element) || '');
        const box = computeBox(element);
        
        // Skip inline generic elements with just text
        if (role === 'generic' && box.inline && 
            element.childNodes.length === 1 && 
            element.childNodes[0].nodeType === 3) {
            return null;
        }
        
        const result = {
            role: role,
            name: name,
            children: [],
            props: {},
            element: element,
            box: box,
            receivesPointerEvents: receivesPointerEvents(element),
            active: active,
            scrollable: (element.scrollHeight > element.clientHeight + 4) || (element.scrollWidth > element.clientWidth + 4)
        };
        
        computeAriaIndex(result);
        
        // Add ARIA properties based on role
        const checkedRoles = ['checkbox', 'menuitemcheckbox', 'menuitemradio', 'radio', 'switch'];
        if (checkedRoles.includes(role)) {
            const checked = getAriaChecked(element);
            if (checked !== undefined) result.checked = checked;
        }
        
        const disabledRoles = ['button', 'input', 'select', 'textarea'];
        if (disabledRoles.includes(role) || role.includes('menuitem')) {
            const disabled = getAriaDisabled(element);
            if (disabled !== undefined) result.disabled = disabled;
        }
        
        const expandedRoles = ['button', 'combobox', 'gridcell', 'link', 'menuitem', 'row', 'tab', 'treeitem'];
        if (expandedRoles.includes(role)) {
            const expanded = getAriaExpanded(element);
            if (expanded !== undefined) result.expanded = expanded;
        }
        
        const levelRoles = ['heading', 'listitem', 'row', 'treeitem'];
        if (levelRoles.includes(role)) {
            const level = getAriaLevel(element);
            if (level !== undefined) result.level = level;
        }
        
        const pressedRoles = ['button'];
        if (pressedRoles.includes(role)) {
            const pressed = getAriaPressed(element);
            if (pressed !== undefined) result.pressed = pressed;
        }
        
        const selectedRoles = ['gridcell', 'option', 'row', 'tab', 'treeitem'];
        if (selectedRoles.includes(role)) {
            const selected = getAriaSelected(element);
            if (selected !== undefined) result.selected = selected;
        }
        
        // Special handling for input/textarea values
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
            if (element.type !== 'checkbox' && element.type !== 'radio' && element.type !== 'file') {
                result.children = [element.value || ''];
            }
        }
        if (element.tagName === 'SELECT') {
            result.children = [[...element.selectedOptions].map(o => o.label).join(', ')];
        }
        
        return result;
    }

    // Main visitor function
    function visit(ariaNode, node, parentElementVisible, visited) {
        if (visited.has(node)) return;
        visited.add(node);
        
        // Handle text nodes
        if (node.nodeType === 3) { // TEXT_NODE
            if (!parentElementVisible) return;
            
            const text = node.nodeValue;
            // Skip text inside textbox
            if (ariaNode.role !== 'textbox' && text) {
                ariaNode.children.push(text);
            }
            return;
        }
        
        // Only process element nodes
        if (node.nodeType !== 1) return; // ELEMENT_NODE
        
        const element = node;
        
        // Check visibility
        const isElementVisibleForAria = !isElementHiddenForAria(element);
        let visible = isElementVisibleForAria || isElementVisible(element);
        
        // Skip if not visible for ARIA
        if (!visible) return;
        
        // Handle aria-owns
        const ariaChildren = [];
        if (element.hasAttribute('aria-owns')) {
            const ids = element.getAttribute('aria-owns').split(/\s+/);
            for (const id of ids) {
                const ownedElement = document.getElementById(id);
                if (ownedElement) ariaChildren.push(ownedElement);
            }
        }
        
        // Convert to aria node
        const childAriaNode = toAriaNode(element);
        if (childAriaNode) {
            ariaNode.children.push(childAriaNode);
        }
        
        // Process element (add CSS content, children, etc.)
        processElement(childAriaNode || ariaNode, element, ariaChildren, visible, visited);
    }

    function processElement(ariaNode, element, ariaChildren, parentElementVisible, visited) {
        const style = window.getComputedStyle(element);
        const display = style ? style.display : 'inline';
        const treatAsBlock = (display !== 'inline' || element.nodeName === 'BR') ? ' ' : '';
        
        if (treatAsBlock) {
            ariaNode.children.push(treatAsBlock);
        }
        
        // Add ::before content
        const beforeContent = getCSSContent(element, '::before');
        if (beforeContent) {
            ariaNode.children.push(beforeContent);
        }
        
        // Process shadow DOM slots
        if (element.nodeName === 'SLOT') {
            const assignedNodes = element.assignedNodes();
            for (const child of assignedNodes) {
                visit(ariaNode, child, parentElementVisible, visited);
            }
        } else {
            // Process regular children
            for (let child = element.firstChild; child; child = child.nextSibling) {
                if (!child.assignedSlot) {
                    visit(ariaNode, child, parentElementVisible, visited);
                }
            }
            
            // Process shadow root
            if (element.shadowRoot) {
                for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
                    visit(ariaNode, child, parentElementVisible, visited);
                }
            }
            
            // Same-origin iframe content (cross-origin is skipped by the browser).
            if (element.tagName === 'IFRAME') {
                try {
                    const doc = element.contentDocument;
                    if (doc && doc.body) {
                        for (let child = doc.body.firstChild; child; child = child.nextSibling) {
                            visit(ariaNode, child, parentElementVisible, visited);
                        }
                    }
                } catch (e) { /* cross-origin */ }
            }
        }
        
        // Process aria-owns children
        for (const child of ariaChildren) {
            visit(ariaNode, child, parentElementVisible, visited);
        }
        
        // Add ::after content
        const afterContent = getCSSContent(element, '::after');
        if (afterContent) {
            ariaNode.children.push(afterContent);
        }
        
        if (treatAsBlock) {
            ariaNode.children.push(treatAsBlock);
        }
        
        // Remove redundant children
        if (ariaNode.children.length === 1 && ariaNode.name === ariaNode.children[0]) {
            ariaNode.children = [];
        }
        
        // Add special props
        if (ariaNode.role === 'link' && element.hasAttribute('href')) {
            ariaNode.props.url = element.getAttribute('href');
        }
        
        if (ariaNode.role === 'textbox' && element.hasAttribute('placeholder')) {
            const placeholder = element.getAttribute('placeholder');
            if (placeholder !== ariaNode.name) {
                ariaNode.props.placeholder = placeholder;
            }
        }
    }

    // Normalize string children
    function normalizeStringChildren(ariaNode) {
        const normalizedChildren = [];
        let buffer = [];
        
        function flushBuffer() {
            if (buffer.length === 0) return;
            const text = normalizeWhiteSpace(buffer.join(''));
            if (text) {
                normalizedChildren.push(text);
            }
            buffer = [];
        }
        
        for (const child of ariaNode.children || []) {
            if (typeof child === 'string') {
                buffer.push(child);
            } else {
                flushBuffer();
                normalizeStringChildren(child);
                normalizedChildren.push(child);
            }
        }
        flushBuffer();
        
        ariaNode.children = normalizedChildren;
        
        // Remove if child equals name
        if (ariaNode.children.length === 1 && ariaNode.children[0] === ariaNode.name) {
            ariaNode.children = [];
        }
    }

    // Normalize generic roles (remove unnecessary generic wrappers)
    function normalizeGenericRoles(node) {
        function normalizeChildren(node) {
            const result = [];
            
            for (const child of node.children || []) {
                if (typeof child === 'string') {
                    result.push(child);
                    continue;
                }
                
                const normalized = normalizeChildren(child);
                result.push(...normalized);
            }
            
            // Remove generic that encloses single element
            const removeSelf = node.role === 'generic' && !node.name && 
                              result.length <= 1 && 
                              result.every(c => typeof c !== 'string' && c.index !== undefined);
            
            if (removeSelf) {
                return result;
            }
            
            node.children = result;
            return [node];
        }
        
        normalizeChildren(node);
    }

    // Serialize ariaNode to plain object (remove Element references)
    function serializeAriaNode(ariaNode) {
        const result = {
            role: ariaNode.role,
            name: ariaNode.name,
            children: [],
            props: ariaNode.props
        };
        
        // Include index if present; rect only where the agent may need to aim a scroll.
        if (ariaNode.index !== undefined) {
            result.index = ariaNode.index;
            if (ariaNode.scrollable || ['listbox', 'option'].includes(ariaNode.role)) {
                const b = ariaNode.box.rect;
                result.rect = { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.width), h: Math.round(b.height) };
            }
        }
        if (ariaNode.active) result.active = true;
        if (ariaNode.checked !== undefined) result.checked = ariaNode.checked;
        if (ariaNode.disabled !== undefined) result.disabled = ariaNode.disabled;
        if (ariaNode.expanded !== undefined) result.expanded = ariaNode.expanded;
        if (ariaNode.level !== undefined) result.level = ariaNode.level;
        if (ariaNode.pressed !== undefined) result.pressed = ariaNode.pressed;
        if (ariaNode.selected !== undefined) result.selected = ariaNode.selected;
        if (ariaNode.scrollable) result.scrollable = true;
        
        // Serialize children
        for (const child of ariaNode.children) {
            if (typeof child === 'string') {
                result.children.push(child);
            } else {
                result.children.push(serializeAriaNode(child));
            }
        }
        
        return result;
    }

    // Collect selectors and iframe indices
    function collectSelectorsAndIframes(ariaNode, selectors, iframeIndices) {
        if (ariaNode.index !== undefined && ariaNode.element) {
            // Store CSS selector for element at its index position
            const selector = buildSelector(ariaNode.element);
            // Ensure selectors array is large enough
            while (selectors.length <= ariaNode.index) {
                selectors.push('');
            }
            selectors[ariaNode.index] = selector;
            // Register index->element so the Rust act/TARGET helpers can resolve it.
            const cache = window.__zepiBrowser ||= {ids:new WeakMap(), nodes:new Map(), next:1};
            cache.nodes.set(ariaNode.index, ariaNode.element);
            (cache.meta ||= new Map()).set(ariaNode.index, { sel: selector, text: (ariaNode.name || '').trim(), role: ariaNode.role });
            
            if (ariaNode.role === 'iframe') {
                iframeIndices.push(ariaNode.index);
            }
        }
        
        for (const child of ariaNode.children) {
            if (typeof child !== 'string') {
                collectSelectorsAndIframes(child, selectors, iframeIndices);
            }
        }
    }

    // Build CSS selector for element
    function buildSelector(element) {
        if (element.id) {
            return '#' + element.id;
        }
        
        const path = [];
        let current = element;
        
        while (current && current !== document.body) {
            let selector = current.tagName.toLowerCase();
            
            if (current.className && typeof current.className === 'string') {
                const classes = current.className.trim().split(/\s+/);
                if (classes.length > 0 && classes[0]) {
                    selector += '.' + classes[0];
                }
            }
            
            // Add nth-child if needed for uniqueness
            const parent = current.parentElement;
            if (parent) {
                const siblings = Array.from(parent.children);
                const index = siblings.indexOf(current);
                if (siblings.filter(s => s.tagName === current.tagName).length > 1) {
                    selector += ':nth-child(' + (index + 1) + ')';
                }
            }
            
            path.unshift(selector);
            current = current.parentElement;
        }
        
        return path.join(' > ');
    }

    // Main execution
    try {
        const rootElement = document.body || document.documentElement;
        const visited = new Set();
        
        // Reset index counter
        currentIndex = 0;
        
        // Create root fragment node
        const snapshot = {
            role: 'fragment',
            name: '',
            children: [],
            props: {},
            element: rootElement,
            box: computeBox(rootElement),
            receivesPointerEvents: true
        };
        
        // Visit the DOM tree
        visit(snapshot, rootElement, true, visited);
        
        // Normalize
        normalizeStringChildren(snapshot);
        normalizeGenericRoles(snapshot);
        
        // Collect selectors and iframe indices
        const selectors = [];
        const iframeIndices = [];
        collectSelectorsAndIframes(snapshot, selectors, iframeIndices);

        // SPA frameworks re-render menus (virtual scroll) and replace nodes; keep selectors
        // so a stale index can be re-resolved to the live element at action time.
        const cache = window.__zepiBrowser ||= {ids:new WeakMap(), nodes:new Map(), next:1};
        cache.selectors = selectors;
        window.__zepiResolve = (node) => {
            let e = cache.nodes.get(node);
            if (e && e.isConnected) return e;
            const m = cache.meta && cache.meta.get(node);
            if (m && m.sel) { const q = document.querySelector(m.sel); if (q && q.isConnected) { cache.nodes.set(node, q); return q; } }
            // Virtual-scroll menus replace nodes and shift nth-child paths; re-locate by visible text.
            if (m && m.text && ['option','tab','menuitem','menuitemcheckbox','menuitemradio','button','link','treeitem'].includes(m.role)) {
                const cand = [...document.querySelectorAll('[role="' + m.role + '"]')]
                    .find(x => (x.textContent || '').trim() === m.text || (x.getAttribute('aria-label') || '') === m.text);
                if (cand) { cache.nodes.set(node, cand); return cand; }
            }
            return null;
        };
        
        // Live name/role targeting (Playwright getByRole semantics): resolve at action time,
        // immune to stale indices from SPA re-renders.
        window.__zepiFind = (name, role) => {
            const want = (name || '').toLowerCase();
            if (!want) return null;
            const c = window.__zepiBrowser ||= {ids:new WeakMap(), nodes:new Map(), next:1};
            c.domIds ||= new WeakMap(); c.domNext ||= 1;
            const els = [...document.querySelectorAll('a[href],button,input,textarea,select,summary,[contenteditable="true"],[role]')];
            for (const e of els) {
                if (role && (e.getAttribute('role') || '') !== role) continue;
                if (!e.checkVisibility || !e.checkVisibility({checkOpacity:true,checkVisibilityCSS:true})) continue;
                const n = (getElementAccessibleName(e) || '').toLowerCase();
                if (n && n.includes(want)) {
                    if (!c.domIds.has(e)) c.domIds.set(e, c.domNext++);
                    const id = c.domIds.get(e);
                    c.nodes.set(id, e);
                    (c.meta ||= new Map()).set(id, {text:getElementAccessibleName(e), role:getAriaRole(e)});
                    return id;
                }
            }
            return null;
        };
        
        // Serialize, then flatten to only interactive elements (compact for the model).
        const serialized = serializeAriaNode(snapshot);
        const elements = [];
        (function collect(n) {
            if (n.index !== undefined) {
                const el = { index: n.index, role: n.role, name: n.name };
                const v = (n.children || []).find(c => typeof c === 'string');
                if (v !== undefined) el.value = v;
                for (const k of ['checked', 'selected', 'expanded', 'disabled', 'scrollable']) if (n[k] !== undefined) el[k] = n[k];
                if (n.rect) el.rect = n.rect;
                elements.push(el);
            }
            for (const c of n.children || []) if (typeof c === 'object') collect(c);
        })(serialized);
        
        // When a menu/dialog is open, focus the snapshot on it (smaller context, options visible).
        const overlay = [...document.querySelectorAll('[role="dialog"],.q-dialog,.q-menu')]
            .find(o => o.checkVisibility && o.checkVisibility({checkOpacity:true,checkVisibilityCSS:true}) && o.getBoundingClientRect().height > 0);
        let outElements = elements;
        let outText = (document.body.innerText || '').replace(/\n{3,}/g, '\n\n').slice(0, 4000);
        if (overlay) {
            outElements = elements.filter(el => {
                const n = cache.nodes.get(el.index);
                return (n && overlay.contains(n)) || el.role === 'combobox';
            });
            outText = (overlay.innerText || '').replace(/\n{3,}/g, '\n\n').slice(0, 4000);
        }
        
        return {
            url: location.href,
            title: document.title,
            scroll: { y: scrollY, height: document.documentElement.scrollHeight },
            text: outText,
            overlay: !!overlay,
            elements: outElements
        };
        
    } catch (error) {
        return { error: error.toString(), elements: [] };
    }
})()
