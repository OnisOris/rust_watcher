import QtQuick
import QtQuick.Controls
import "./components"

ApplicationWindow {
    id: window
    property string titleText: "Person"
    signal accepted(string value)

    PersonCard {
        id: card
        name: titleText
    }

    Button {
        text: card.name
        onClicked: loadPerson()
    }

    function loadPerson() {
        fetch("/api/person")
    }
}
