// The critical journey: pick an item, read its detail. Text is set with textContent, never as HTML.
(function () {
  var data = window.PROTOTYPE_DATA || { items: [] };
  var root = document.getElementById("app");
  var list = document.createElement("ul");
  list.className = "items";
  var detail = document.createElement("section");
  detail.className = "detail";
  detail.hidden = true;
  function show(item, button) {
    var buttons = list.querySelectorAll("button");
    for (var i = 0; i < buttons.length; i++) buttons[i].setAttribute("aria-current", "false");
    button.setAttribute("aria-current", "true");
    detail.hidden = false;
    detail.replaceChildren();
    var h = document.createElement("h2");
    h.textContent = item.title;
    var p = document.createElement("p");
    p.textContent = item.summary;
    var tag = document.createElement("p");
    tag.className = "tag";
    tag.textContent = "Source: " + item.source;
    detail.append(h, p, tag);
  }
  data.items.forEach(function (item) {
    var li = document.createElement("li");
    var b = document.createElement("button");
    b.type = "button";
    b.textContent = item.title;
    b.addEventListener("click", function () { show(item, b); });
    li.appendChild(b);
    list.appendChild(li);
  });
  root.append(list, detail);
})();
