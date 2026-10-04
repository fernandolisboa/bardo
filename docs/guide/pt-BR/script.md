---
id: script
title: Roteiro
group: production
place: projects/script
tour: script
---

# Roteiro

O roteiro é o que o narrador vai dizer. O Claude escreve, você edita, e todas as etapas seguintes partem dele. Ele fica na etapa [Roteiro](bardo:go/projects/script) de um projeto, com o prompt de música do vídeo logo abaixo. [Mostre a etapa para mim](bardo:tour/script).

<a id="write"></a>
## Gere e depois edite

**Gerar roteiro** pede ao Claude um roteiro montado a partir do **modelo de roteiro** ([Modelos](templates.md)), do canal (nome, nicho, temas, visual e idioma), do tema aprovado e do tom e do estilo de roteiro do narrador. A geração roda como tarefa, então você pode continuar trabalhando; é preciso uma chave do Claude ([Suas chaves de API](api-keys.md#providers)).

Depois que o roteiro chega aqui, ele é seu: digite direto nele. Cabem até 60.000 caracteres, e a contagem de palavras ao lado do título dá uma ideia da duração do vídeo.

<a id="review"></a>
## Uma nova versão para revisar

**Gerar de novo** pede um roteiro inteiramente novo sem mexer no seu. O novo espera acima dele como **Novo roteiro para revisar**:

- **Aceitar o novo roteiro** substitui o atual, inclusive as suas edições.
- **Manter o atual** descarta o novo.

Até você decidir, a etapa Roteiro mostra **Nova versão para revisar**.

<a id="save"></a>
## Salvar as suas edições

**Salvar** guarda o que você digitou, e o roteiro fica marcado como **Editado**. **Descartar alterações** volta ao roteiro salvo por último. O que foi feito a partir de um roteiro anterior, como a narração, passa a aparecer como **Desatualizada** ([Narração](narration.md#stale)).

<a id="cost"></a>
## Quanto custa

Antes de gerar, a estimativa sob os botões diz quanto a chamada vai custar pelas tarifas do provedor. Se a chamada passaria de um dos seus orçamentos, o Bardo pergunta antes de começar. Quanto o projeto já gastou fica sob o nome dele ([Projetos e etapas](projects.md#cost)).

<a id="details"></a>
## De onde ele veio

**Detalhes** lista o provedor, o modelo de IA, o modelo de roteiro e a versão dele, os tokens usados e quando o roteiro foi gerado. **Mostrar prompt** mostra as instruções e o prompt exatamente como foram enviados. Um novo roteiro à espera de revisão tem os próprios detalhes.

<a id="music"></a>
## O prompt de música

Abaixo do roteiro, **Gerar prompt de música** pede ao Claude um prompt para a sua ferramenta de música, a partir do **modelo de prompt de música**, do canal, do tema e da duração do vídeo. O Bardo não faz música: **Copie** o prompt, faça a faixa numa ferramenta que você tenha direito de usar e importe-a na aba Mídia do editor. Edite o prompt e **Salve**; **Gerar de novo** o substitui, inclusive as suas edições.
