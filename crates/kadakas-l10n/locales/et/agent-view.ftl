## agent-view: agent_view_ui.rs (AGR-02, AGR-03: mida agent vaates näitab)

agent-view-name = Agent
agent-view-back-count = Tagasi ({ $count })
agent-view-back-hint = Tagasi vaatesse, mis sul oli enne, kui agent seda liigutas ({ $key })
agent-view-next = Järgmine osuti
agent-view-next-hint = Suumi agendi järgmise osuti juurde ({ $key })
agent-view-pointers = Agendi osutid ({ $count })
agent-view-dismiss-mark = ×
agent-view-dismiss = Sulge
agent-view-dismiss-hint = Eemalda see osuti
agent-view-clear-all = Eemalda kõik
agent-view-zoom-hint = Suumi selle osuti juurde
agent-view-what-objects =
    { $count ->
        [one] 1 objekt
       *[other] { $count } objekti
    }
agent-view-what-box = Ala
agent-view-what-place = Koht
agent-view-row = { $number }. { $what } ({ $color })
agent-view-row-text = { $number }. { $text } ({ $color })
agent-view-color-agent = tumehall romb
agent-view-color-red = punane kolmnurk
agent-view-color-blue = sinine ruut
agent-view-color-green = roheline ring
agent-view-color-orange = oranž pluss
agent-view-people-pointing =
    { $count ->
        [one] osutab 1 osutiga
       *[other] osutab { $count } osutiga
    }
agent-view-people-hint = Agent liigutab vaadet ja osutab. Sa saad alati minna Tagasi või osuti sulgeda.
agent-view-restored = Tagasi vaatesse enne agendi muudatust.
agent-view-nothing-back = Agent pole sinu vaadet muutnud.
agent-view-gone = See osuti on kadunud.
agent-view-allow-move = Agent võib minu vaadet liigutada
agent-view-allow-move-hint = Väljas: agent saab endiselt osutada ja lugeda, aga vaate liigutamise päring ebaõnnestub ja midagi ei liigu.
agent-view-close = Peida
agent-view-close-hint = Peida see paneel. See tuleb tagasi agendi järgmise osuti või vaatemuutusega.
agent-view-showed-view = Agent näitas vaadet { $name } (ainult kaamera; kihid jäid samaks)
