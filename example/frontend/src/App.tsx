import { useEffect, useState } from 'react'

interface PersonDto {
  name: string
  age: number
  mood: 'Happy' | 'Focused'
}

function usePerson(name: string) {
  const [person, setPerson] = useState<PersonDto | null>(null)

  useEffect(() => {
    async function loadPerson() {
      const response = await fetch('/api/person')
      const payload = await response.json() as PersonDto
      setPerson(payload)
    }

    loadPerson()
  }, [])

  async function renamePerson(nextName: string) {
    const response = await fetch(`/api/person/${nextName}`, { method: 'POST' })
    const payload = await response.json() as PersonDto
    setPerson(payload)
  }

  return { person, renamePerson, name }
}

export function PersonCard() {
  const { person, renamePerson } = usePerson('Ada')

  return (
    <section>
      <h1>{person?.name ?? 'Loading person'}</h1>
      <button onClick={() => renamePerson('Grace')}>Rename</button>
    </section>
  )
}
